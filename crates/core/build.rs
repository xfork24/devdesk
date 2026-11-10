#[path = "../../tests/support/schemas.rs"]
mod schemas;
use std::{
    collections::BTreeSet,
    env, fs,
    path::{Path, PathBuf},
};

fn collect(root: &Path, dir: &Path, files: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("template resource directory") {
        let path = entry.unwrap().path();
        if path.is_dir() {
            collect(root, &path, files);
        } else if matches!(
            path.extension().and_then(|v| v.to_str()),
            Some("json" | "md")
        ) {
            files.push(path.strip_prefix(root).unwrap().to_owned());
        }
    }
}
fn safe_file(root: &Path, name: &str) {
    assert!(
        !name.is_empty()
            && Path::new(name)
                .components()
                .all(|p| matches!(p, std::path::Component::Normal(_))),
        "invalid template path: {name}"
    );
    assert!(
        root.join(name).is_file(),
        "missing template resource: {name}"
    );
}
fn main() {
    let root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap())
        .join("../../resources/configuration");
    println!("cargo:rerun-if-changed={}", root.display());
    let manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(root.join("manifest.json")).unwrap())
            .expect("template manifest JSON");
    schemas::SCHEMAS.assert_valid("template-manifest.schema.json", &manifest);
    println!(
        "cargo:rerun-if-changed={}",
        root.join("../../schemas").display()
    );
    assert_eq!(manifest["schema_version"], 1);
    let mut listed = BTreeSet::new();
    let mut keys = BTreeSet::new();
    let mut references = Vec::new();
    for (kind, paths) in manifest["types"].as_object().expect("template types") {
        for path in paths.as_array().expect("template type file list") {
            let path = path.as_str().unwrap();
            safe_file(&root, path);
            assert!(
                listed.insert(path.to_owned()),
                "duplicate template file: {path}"
            );
            let entry: serde_json::Value =
                serde_json::from_str(&fs::read_to_string(root.join(path)).unwrap())
                    .expect("template JSON");
            schemas::SCHEMAS.assert_valid("template-entry.schema.json", &entry);
            if kind != "projections" && kind != "initial" && kind != "mcp" && kind != "http" {
                assert_eq!(
                    entry["record"]["kind"],
                    kind.as_str(),
                    "template type mismatch: {path}"
                );
            }
            if kind == "mcp" || kind == "http" {
                assert_eq!(entry["record"]["kind"], "integration");
                assert_eq!(
                    entry["record"]["data"]["source"]["kind"],
                    if kind == "mcp" { "mcp" } else { "http_api" }
                );
            }
            let mut expanded = entry["record"].clone();
            for (pointer, file) in entry["text_files"].as_object().unwrap() {
                *expanded
                    .pointer_mut(pointer)
                    .expect("template body pointer") = serde_json::json!(
                    fs::read_to_string(root.join(file.as_str().unwrap())).unwrap()
                );
            }
            if entry["storage"] == "governance" {
                expanded["content_hash"] =
                    serde_json::json!(devdesk_domain::configuration_hash(&expanded["data"]));
                schemas::SCHEMAS
                    .assert_valid("rpc.schema.json#/$defs/ConfigurationRecord", &expanded);
                let schema = match expanded["kind"].as_str().unwrap() {
                    "role" => Some("role.schema.json"),
                    "model" => Some("model.schema.json"),
                    "capability" => Some("capability.schema.json"),
                    "agent" => Some("agent-composition.schema.json"),
                    "workflow" => Some("workflow.schema.json"),
                    "tool" => Some("tool.schema.json"),
                    "delivery_type" => Some("delivery-type.schema.json"),
                    "integration" => Some("integration.schema.json"),
                    _ => None,
                };
                if let Some(schema) = schema {
                    schemas::SCHEMAS.assert_valid(schema, &expanded["data"]);
                }
            }
            assert!(
                keys.insert(entry["template_key"].as_str().unwrap().to_owned()),
                "duplicate template key"
            );
            for file in entry["text_files"].as_object().unwrap().values() {
                safe_file(&root, file.as_str().unwrap());
            }
            for reference in entry["references"].as_object().unwrap().values() {
                references.push(reference["template_ref"].as_str().unwrap().to_owned());
            }
        }
    }
    for reference in references {
        assert!(
            keys.contains(&reference),
            "missing template dependency: {reference}"
        );
    }
    safe_file(&root, manifest["defaults"].as_str().unwrap());
    let mut files = Vec::new();
    collect(&root, &root, &mut files);
    files.sort();
    let mut code = String::from("pub const TEMPLATE_FILES: &[(&str, &str)] = &[\n");
    for file in files {
        code.push_str(&format!(
            "({:?}, include_str!({:?})),\n",
            file.to_str().unwrap(),
            root.join(&file).to_str().unwrap()
        ));
    }
    code.push_str("];\n");
    fs::write(
        PathBuf::from(env::var("OUT_DIR").unwrap()).join("template_files.rs"),
        code,
    )
    .unwrap();
}
