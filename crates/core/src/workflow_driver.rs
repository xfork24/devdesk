//! Rust and Harness drivers use the same explicit control kernel and persisted worker intents.
use crate::*;

pub(crate) fn validate_selection(
    _s: &EngineeringState,
    value: &Value,
    _scope: &Scope,
) -> Result<OrchestrationSelection> {
    let selection: OrchestrationSelection = serde_json::from_value(value.clone())
        .map_err(|e| err("invalid_configuration", e.to_string()))?;
    selection.validate()?;
    Ok(selection)
}
