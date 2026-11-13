//! Offline access to every published schema, including sibling-file references.
use jsonschema::{Registry, Validator};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path, sync::LazyLock};

pub struct Catalog {
    // Schema enumeration is used by the contract test binary, not every importing suite.
    #[allow(dead_code)]
    pub documents: BTreeMap<String, Value>,
    registry: Registry<'static>,
}
pub static SCHEMAS: LazyLock<Catalog> = LazyLock::new(|| {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas");
    let documents: BTreeMap<String, Value> = std::fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .map(|path| {
            let value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            (
                path.file_name().unwrap().to_str().unwrap().to_owned(),
                value,
            )
        })
        .collect();
    let mut registry = Registry::new();
    for (name, document) in &documents {
        registry = registry
            .add(
                format!("https://devdesk.invalid/schemas/{name}"),
                document.clone(),
            )
            .unwrap();
    }
    Catalog {
        documents,
        registry: registry.prepare().unwrap(),
    }
});
impl Catalog {
    pub fn validator(&self, reference: &str) -> Validator {
        jsonschema::options()
            .offline()
            .with_registry(&self.registry)
            .build(&json!({"$ref":format!("https://devdesk.invalid/schemas/{reference}")}))
            .unwrap_or_else(|error| panic!("{reference}: {error}"))
    }
    pub fn assert_valid(&self, reference: &str, instance: &Value) {
        let validator = self.validator(reference);
        let errors: Vec<_> = validator
            .iter_errors(instance)
            .map(|e| e.to_string())
            .collect();
        assert!(errors.is_empty(), "{reference}: {}", errors.join("\n"));
    }
}
