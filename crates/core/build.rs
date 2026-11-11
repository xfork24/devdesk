#[path = "../../tests/support/schemas.rs"]
mod schemas;
use std::{
    env, fs,
    path::{Path, PathBuf},
};
fn collect(root: &Path, dir: &Path, files: &mut Vec<PathBuf>) {
    for e in fs::read_dir(dir).expect("configuration directory") {
        let path = e.unwrap().path();
        if path.is_dir() {
            collect(root, &path, files)
        } else if matches!(
            path.extension().and_then(|v| v.to_str()),
            Some("json" | "md")
        ) {
            files.push(path.strip_prefix(root).unwrap().to_owned())
        }
    }
}
fn main() {
    let root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap())
        .join("../../resources/configuration");
    println!("cargo:rerun-if-changed={}", root.display());
    println!(
        "cargo:rerun-if-changed={}",
        root.join("../../schemas").display()
    );
    let manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(root.join("manifest.json")).unwrap()).unwrap();
    schemas::SCHEMAS.assert_valid("template-manifest.schema.json", &manifest);
    for (kind, paths) in manifest["types"].as_object().unwrap() {
        for path in paths.as_array().unwrap() {
            let source: serde_json::Value = serde_json::from_str(
                &fs::read_to_string(root.join(path.as_str().unwrap())).unwrap(),
            )
            .unwrap();
            schemas::SCHEMAS.assert_valid("source-definition.schema.json", &source);
            assert_eq!(source["kind"], kind.as_str(), "source kind mismatch");
        }
    }
    let mut files = vec![];
    collect(&root, &root, &mut files);
    files.sort();
    let mut code = String::from("pub const TEMPLATE_FILES: &[(&str, &str)] = &[\n");
    for f in files {
        code.push_str(&format!(
            "({:?}, include_str!({:?})),\n",
            f.to_str().unwrap(),
            root.join(&f).to_str().unwrap()
        ));
    }
    code.push_str("];\n");
    fs::write(
        PathBuf::from(env::var("OUT_DIR").unwrap()).join("template_files.rs"),
        code,
    )
    .unwrap();
}
