//! Export current factory recipes for native acceptance without duplicating the compiler.
use devdesk_domain::HARNESS_PROVIDERS;
use serde_json::json;
use std::collections::BTreeMap;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut recipes = BTreeMap::new();
    for provider in HARNESS_PROVIDERS {
        let recipe = devdesk_adapters::packaged_runtime(*provider)?;
        recipes.insert(provider.as_str(), json!(recipe));
    }
    println!("{}", serde_json::to_string(&recipes)?);
    Ok(())
}
