//! Explicit compatibility input for tests of frozen, retired engineering sources.
//! This file and its fixture are never loaded by the product factory catalog.
use devdesk_core::Engine;
use devdesk_domain::*;
use serde_json::json;

pub fn install(e: &Engine) {
    let sources: Vec<SourceDefinition> = serde_json::from_str(include_str!(
        "../fixtures/configuration/retired-engineering.json"
    ))
    .unwrap();
    e.store
        .seed_fixture_state(None, &mut |state| {
            for source in &sources {
                let row = source.clone().into_record()?;
                if state.configuration(&row.id).is_err() {
                    state.governance.push(row);
                }
            }
            Ok(json!(null))
        })
        .unwrap();
}
