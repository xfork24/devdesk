//! Delivery definitions describe real workflow materials; they do not fabricate artifacts.
use crate::*;

pub(crate) fn definitions(
    s: &EngineeringState,
    step: &WorkflowStep,
    scope: &Scope,
) -> Result<Vec<ConfigurationRecord>> {
    let ids: std::collections::BTreeSet<_> = step
        .delivery_outputs
        .values()
        .map(|v| &v.delivery_ref)
        .chain(step.delivery_inputs.values().map(|v| &v.delivery_ref))
        .collect();
    ids.into_iter()
        .map(|id| {
            let record = crate::governance::reference(s, id, "delivery_type", scope)?;
            record.decode::<DeliveryTypeDefinition>()?.validate()?;
            Ok(record.clone())
        })
        .collect()
}
