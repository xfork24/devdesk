use devdesk_adapters::*;
use devdesk_core::*;
use devdesk_domain::*;
use serde_json::json;
use std::sync::Arc;
#[path = "../../../tests/support/runtime.rs"]
mod runtime;
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
fn source(version: &str, content: &str) -> ConfigurationRecord {
    SourceDefinition{r#type:"runtime_resource".into(),identity:format!("resource-{version}"),revision:1,name:version.into(),scope:Scope::global(),refs:References::new(),
        data:json!({"provider":"codex","version":version,"license":"test","files":[{"path":"Dockerfile","content":content}]})}.into_record().unwrap()
}
#[test]
fn labels_and_native_catalog_do_not_determine_recipe_identity() {
    let a: RuntimeResourceDefinition = source("alpha", "FROM scratch\n").decode().unwrap();
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
    for label in ["alpha", "beta"] {
        let record = source(label, &format!("FROM scratch\nLABEL test={label}\n"));
        let src = record.source();
        let change = json!({"operations":[{"type":"create","source":src},{"type":"create","source":{"type":"harness","identity":format!("h-{label}"),"revision":1,"name":label,"scope":"global","runtime_resource_ref":record.id,"model_providers":[],"data":{"provider":"codex","runtime_version":"unrelated-display","capabilities":["dynamic-tools"]}}}]});
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
    let bad=e.execute("harness.create",Command{command_id:id(),target:None,expected_revision:None,payload:json!({"name":"forged","model_providers":[],"data":{"provider":"dsh","runtime_version":"any-label","capabilities":["dynamic-tools"]}})});
    assert!(bad.is_err());
}
#[test]
#[ignore = "explicit local Docker protocol acceptance; no model input"]
fn real_two_recipes_probe_without_inference() {
    let dir = tempfile::tempdir().unwrap();
    let (store, _) = setup(dir.path());
    let mut installations = Vec::new();
    for label in ["alpha", "beta"] {
        let record = source(
            label,
            &format!("FROM devdesk/codex:0.167.0\nLABEL devdesk.acceptance={label}\n"),
        );
        let i = store.prepare_runtime_resource(&record, |_| {}).unwrap();
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
    let source = a.default_runtime_resource(HarnessProvider::Codex).unwrap();
    let old = a.prepare_runtime_resource(&source, |_| {}).unwrap();
    let new = b.prepare_runtime_resource(&source, |_| {}).unwrap();
    a.require_runtime_verification(&old).unwrap();
    b.require_runtime_verification(&new).unwrap();
    assert_eq!(
        a.prepare_runtime_resource(&source, |_| {})
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
