#[path = "../../scripts/rust/schema_validation.rs"]
mod schemas;
use std::{
    env, fs,
    path::{Path, PathBuf},
};
fn main() {
    let root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("../../configurations");
    let core = root.join("core");
    let fixture = root.join("../tests/fixtures/configuration/engineering-catalog.json");
    println!("cargo:rerun-if-changed={}", core.display());
    println!(
        "cargo:rerun-if-changed={}",
        root.join("../schemas").display()
    );

    let files = if cfg!(feature = "fixture-catalog") {
        println!("cargo:rerun-if-changed={}", fixture.display());
        let entries: std::collections::BTreeMap<String, serde_json::Value> =
            serde_json::from_str(&fs::read_to_string(fixture).unwrap()).unwrap();
        entries
            .into_iter()
            .map(|(path, contents)| {
                let text = match contents {
                    serde_json::Value::String(text) => text,
                    value => value.to_string(),
                };
                (path, text)
            })
            .collect()
    } else {
        read_catalog(&core)
    };
    let mut code = String::from("pub const TEMPLATE_FILES: &[(&str, &str)] = &[\n");
    for (name, contents) in files {
        code.push_str(&format!("({name:?}, {contents:?}),\n"));
    }
    code.push_str("];\n");
    fs::write(
        PathBuf::from(env::var("OUT_DIR").unwrap()).join("template_files.rs"),
        code,
    )
    .unwrap();
}

fn read_catalog(root: &Path) -> std::collections::BTreeMap<String, String> {
    let read = |path: &str| fs::read_to_string(root.join(path)).unwrap();
    let manifest_text = read("manifest.json");
    let manifest: serde_json::Value = serde_json::from_str(&manifest_text).unwrap();
    schemas::SCHEMAS.assert_valid("template-manifest.schema.json", &manifest);
    let mut files = std::collections::BTreeMap::from([("manifest.json".into(), manifest_text)]);
    for key in ["usage_index", "defaults"] {
        let path = manifest[key].as_str().unwrap();
        files.insert(path.to_owned(), read(path));
    }
    for (kind, paths) in manifest["types"].as_object().unwrap() {
        for path in paths.as_array().unwrap() {
            let path = path.as_str().unwrap();
            let contents = read(path);
            let source: serde_json::Value = serde_json::from_str(&contents).unwrap();
            schemas::SCHEMAS.assert_valid("source-definition.schema.json", &source);
            assert_eq!(source["type"], kind.as_str(), "source type mismatch");
            files.insert(path.to_owned(), contents);
            for (field, value) in source["data"].as_object().unwrap() {
                if field.ends_with("_file") {
                    let body = Path::new(path)
                        .parent()
                        .unwrap()
                        .join(value.as_str().unwrap());
                    let body = body.to_str().unwrap();
                    files.insert(body.to_owned(), read(body));
                }
            }
        }
    }
    files
}
