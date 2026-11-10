//! Delivery definitions describe real workflow materials; they do not fabricate artifacts.
use crate::*;

pub(crate) fn seed(s: &mut EngineeringState) {
    for (key, name, format, body) in [
        (
            "prd",
            "PRD",
            "markdown",
            include_str!("../../../resources/configuration/deliveries/prd.md"),
        ),
        (
            "design",
            "Design Doc",
            "markdown",
            include_str!("../../../resources/configuration/deliveries/design.md"),
        ),
        (
            "diff",
            "Diff",
            "diff",
            include_str!("../../../resources/configuration/deliveries/diff.md"),
        ),
        (
            "test-report",
            "Test Report",
            "test_report",
            include_str!("../../../resources/configuration/deliveries/test-report.md"),
        ),
    ] {
        crate::builtin_defaults::seed(
            s,
            &format!("builtin-delivery-{key}-v1"),
            "delivery_type",
            name,
            json!({"format":format,"body":body}),
        );
    }
}

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
