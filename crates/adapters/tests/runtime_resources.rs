use devdesk_adapters::*;
use devdesk_core::*;
use devdesk_domain::*;
use serde_json::json;
use std::sync::Arc;
#[path = "../../../tests/support/runtime.rs"]
mod runtime;
mod support;
fn setup(path: &std::path::Path) -> (Arc<GlobalStore>, Engine) {
    let store = Arc::new(GlobalStore::open(path).unwrap());
    let e = Engine::new(
        store.clone(),
        Arc::new(FileArtifacts::new(path.into())),
        runtime::adapters(),
    );
    e.bootstrap().unwrap();
    (store, e)
}
fn source(version: &str, content: &str) -> ExecutionRecipe {
    serde_json::from_value(json!({"provider":"codex","version":version,"license":"test","files":[{"path":"Dockerfile","content":content}]})).unwrap()
}
#[test]
fn labels_and_native_catalog_do_not_determine_recipe_identity() {
    let a: ExecutionRecipe = source("alpha", "FROM scratch\n");
    let mut b = a.clone();
    b.version = "other-label".into();
    assert_eq!(a.recipe_digest(), b.recipe_digest());
    b.files[0].content.push_str("LABEL test=second\n");
    assert_ne!(a.recipe_digest(), b.recipe_digest());
    for path in ["/etc/passwd", "../escape", "a/../b", "a\\b", "a//b"] {
        b.files[0].path = path.into();
        assert!(b.validate().is_err(), "{path}");
    }
}
#[test]
fn two_external_recipes_are_valid_and_forged_driver_capability_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let (_, e) = setup(dir.path());
    let state = e.store.snapshot().unwrap();
    let template = state
        .configuration("builtin-harness-codex-managed:1")
        .unwrap();
    for label in ["alpha", "beta"] {
        let mut source = template.source();
        source.identity = format!("h-{label}");
        source.name = label.into();
        source.data["runtime_version"] = json!(label);
        let change = json!({"operations":[{"type":"create","source":source}]});
        let preview = e
            .preview_configuration_change(&serde_json::from_value(change.clone()).unwrap())
            .unwrap();
        e.execute(
            "configuration.change.apply",
            Command {
                command_id: id(),
                target: None,
                expected_revision: None,
                payload: json!({"changeset":change,"preview_hash":preview["preview_hash"]}),
            },
        )
        .unwrap();
    }
    let bad=e.execute("harness.create",Command{command_id:id(),target:None,expected_revision:None,payload:json!({"name":"forged","model_providers":[],"data":{"provider":"dsh","runtime_version":"any-label","capabilities":["orchestration"]}})});
    assert!(bad.is_err());
}
#[test]
#[ignore = "explicit local Docker protocol acceptance; no model input"]
fn real_two_recipes_probe_without_inference() {
    let dir = tempfile::tempdir().unwrap();
    let (store, _) = setup(dir.path());
    let mut installations = Vec::new();
    let image = support::image("codex");
    // BuildKit resolves FROM by a local tag rather than a bare image ID. Give
    // the inspected content its own deterministic tag, independent of releases.
    let base = format!(
        "devdesk/acceptance-base:{}",
        image.trim_start_matches("sha256:")
    );
    assert!(
        std::process::Command::new("docker")
            .args(["tag", &image, &base])
            .status()
            .unwrap()
            .success()
    );
    for label in ["alpha", "beta"] {
        let record = source(
            label,
            &format!("FROM {base}\nLABEL devdesk.acceptance={label}\n"),
        );
        let i = store
            .prepare_execution_recipe(
                &store
                    .default_execution_recipe(HarnessProvider::Codex)
                    .unwrap()
                    .0,
                &record,
                |_| {},
            )
            .unwrap();
        let report = store.runtime_verification(&i).unwrap().unwrap();
        assert_eq!(report["status"], "verified");
        assert_eq!(report["inference_verified"], false);
        assert!(
            report["observations"]
                .as_array()
                .unwrap()
                .contains(&json!("dynamic-tool-registration"))
        );
        installations.push(i);
    }
    assert_ne!(installations[0].artifact, installations[1].artifact);
    assert_ne!(
        installations[0].recipe_digest,
        installations[1].recipe_digest
    );
    assert_eq!(store.runtime_preparation_records().unwrap().len(), 2);
    for i in installations {
        store.require_runtime_verification(&i).unwrap();
    }
}

#[test]
#[ignore = "explicit local Docker rebuild acceptance; no model input"]
fn real_prepared_image_survives_other_dataset_rebuild() {
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    let (a, _) = setup(first.path());
    let (b, _) = setup(second.path());
    let (sandbox, original) = a.default_execution_recipe(HarnessProvider::Codex).unwrap();
    let old = a
        .prepare_execution_recipe(&sandbox, &original, |_| {})
        .unwrap();
    let RuntimeArtifact::DockerImage { image_id } = &old.artifact else {
        panic!()
    };
    // A cache hit yielding the same image cannot prove that retaining an old
    // installation survives another dataset producing a different image.
    let other = source(
        "other-dataset",
        &format!(
            "FROM devdesk/prepared-codex:{}\nLABEL devdesk.acceptance=other-dataset\n",
            image_id.trim_start_matches("sha256:")
        ),
    );
    let new = b
        .prepare_execution_recipe(&sandbox, &other, |_| {})
        .unwrap();
    assert_ne!(old.artifact, new.artifact);
    a.require_runtime_verification(&old).unwrap();
    b.require_runtime_verification(&new).unwrap();
    assert_eq!(
        a.prepare_execution_recipe(&sandbox, &original, |_| {})
            .unwrap()
            .artifact,
        old.artifact
    );
    for i in [&old, &new] {
        let RuntimeArtifact::DockerImage { image_id } = &i.artifact else {
            panic!()
        };
        let retained = format!("devdesk/prepared-codex:{}", &image_id[7..]);
        let out = std::process::Command::new("docker")
            .args(["image", "inspect", "--format", "{{.Id}}", &retained])
            .output()
            .unwrap();
        assert!(out.status.success());
        assert_eq!(String::from_utf8(out.stdout).unwrap().trim(), image_id);
    }
}
