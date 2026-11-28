//! Export current factory recipes for native acceptance without duplicating the compiler.
use devdesk_domain::{HARNESS_PROVIDERS, SourceDefinition, compose_execution_recipe};
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, path::Path};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/core");
    let read = |path: &Path| -> Result<Value, Box<dyn std::error::Error>> {
        Ok(serde_json::from_slice(&fs::read(path)?)?)
    };
    let manifest = read(&root.join("manifest.json"))?;
    let defaults = read(&root.join(manifest["defaults"].as_str().ok_or("missing defaults")?))?;
    let mut sources = BTreeMap::new();
    for kind in ["harness", "sandbox"] {
        for path in manifest["types"][kind]
            .as_array()
            .ok_or("missing sources")?
        {
            let source: SourceDefinition = serde_json::from_value(read(
                &root.join(path.as_str().ok_or("invalid source path")?),
            )?)?;
            let record = source.into_record()?;
            sources.insert(record.id.clone(), record);
        }
    }
    let sandbox = sources
        .get(defaults["sandbox_ref"].as_str().ok_or("missing Sandbox")?)
        .ok_or("default Sandbox is not published")?;
    let mut recipes = BTreeMap::new();
    for provider in HARNESS_PROVIDERS {
        let harness = sources
            .values()
            .find(|source| {
                source.r#type == "harness" && source.data["provider"] == provider.as_str()
            })
            .ok_or("factory Harness is not published")?;
        let recipe = compose_execution_recipe(harness, sandbox)?;
        recipes.insert(provider.as_str(), json!(recipe));
    }
    println!("{}", serde_json::to_string(&recipes)?);
    Ok(())
}
