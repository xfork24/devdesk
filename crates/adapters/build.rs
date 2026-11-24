#[path = "../../scripts/rust/schema_validation.rs"]
mod schemas;
use std::{
    env, fs,
    path::{Path, PathBuf},
};
fn main() {
    let root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("../../configurations");
    println!("cargo:rerun-if-changed={}", root.display());
    println!(
        "cargo:rerun-if-changed={}",
        root.join("../schemas").display()
    );
    let manifest_path = if cfg!(feature = "engineering-defaults") {
        "manifest.json"
    } else {
        "core/manifest.json"
    };
    let manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(root.join(manifest_path)).unwrap()).unwrap();
    schemas::SCHEMAS.assert_valid("template-manifest.schema.json", &manifest);
    let mut files =
        std::collections::BTreeMap::from([("manifest.json".to_string(), root.join(manifest_path))]);
    for key in ["usage_index", "defaults"] {
        let path = manifest[key].as_str().unwrap();
        files.insert(path.to_owned(), root.join(path));
    }
    for (kind, paths) in manifest["types"].as_object().unwrap() {
        for path in paths.as_array().unwrap() {
            let path = path.as_str().unwrap();
            let source: serde_json::Value =
                serde_json::from_str(&fs::read_to_string(root.join(path)).unwrap()).unwrap();
            schemas::SCHEMAS.assert_valid("source-definition.schema.json", &source);
            assert_eq!(source["type"], kind.as_str(), "source type mismatch");
            files.insert(path.to_owned(), root.join(path));
            for (field, value) in source["data"].as_object().unwrap() {
                if field.ends_with("_file") {
                    let body = Path::new(path)
                        .parent()
                        .unwrap()
                        .join(value.as_str().unwrap());
                    files.insert(body.to_str().unwrap().to_owned(), root.join(&body));
                }
            }
        }
    }
    let mut code = String::from("pub const TEMPLATE_FILES: &[(&str, &str)] = &[\n");
    for (name, path) in files {
        code.push_str(&format!("({name:?}, include_str!({path:?})),\n"));
    }
    code.push_str("];\n");
    fs::write(
        PathBuf::from(env::var("OUT_DIR").unwrap()).join("template_files.rs"),
        code,
    )
    .unwrap();
}
