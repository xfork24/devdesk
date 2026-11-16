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
#[derive(Default)]
struct DockerTags(Vec<String>);
impl DockerTags {
    fn track(&mut self, tag: String) -> String {
        self.0.push(tag.clone());
        tag
    }
    fn tag(&mut self, image: &str, tag: String) -> String {
        docker(&["image", "tag", image, &tag]);
        self.track(tag)
    }
}
impl Drop for DockerTags {
    fn drop(&mut self) {
        for tag in self.0.iter().rev() {
            let _ = std::process::Command::new("docker")
                .args(["image", "rm", tag])
                .output();
        }
    }
}
fn docker(args: &[&str]) -> String {
    let output = std::process::Command::new("docker")
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{:?}: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().into()
}
fn recipe_tag(record: &ExecutionRecipe) -> String {
    format!("devdesk/managed-codex:{}", &record.recipe_digest()[..24])
}
fn image_id(installation: &RuntimeInstallation) -> &str {
    let RuntimeArtifact::DockerImage { image_id } = &installation.artifact else {
        panic!()
    };
    image_id
}
fn assert_only_tag(image: &str, expected: &str) {
    let tags: Vec<String> = serde_json::from_str(&docker(&[
        "image",
        "inspect",
        "--format",
        "{{json .RepoTags}}",
        image,
    ]))
    .unwrap();
    assert_eq!(tags, [expected]);
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
    let mut tags = DockerTags::default();
    // BuildKit resolves FROM by a local tag rather than a bare image ID. Give
    // the inspected content a temporary tag, independent of releases.
    let base = tags.tag(&image, format!("devdesk/acceptance-base:{}", id()));
    for label in ["alpha", "beta"] {
        let record = source(
            label,
            &format!("FROM {base}\nLABEL devdesk.acceptance={label}\n"),
        );
        let tag = tags.track(recipe_tag(&record));
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
        assert_only_tag(image_id(&i), &tag);
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
fn real_managed_images_preserve_frozen_installations_without_duplicate_tags() {
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    let (a, _) = setup(first.path());
    let (b, _) = setup(second.path());
    let sandbox = a
        .default_execution_recipe(HarnessProvider::Codex)
        .unwrap()
        .0;
    let mut tags = DockerTags::default();
    let base = tags.tag(
        &support::image("codex"),
        format!("devdesk/acceptance-base:{}", id()),
    );
    let original = source(
        "original",
        &format!("FROM {base}\nLABEL devdesk.acceptance=original\n"),
    );
    let canonical = tags.track(recipe_tag(&original));
    // Independent datasets preparing concurrently share one recipe tag and artifact.
    let old = std::thread::scope(|scope| {
        let first = scope.spawn(|| {
            a.prepare_execution_recipe(&sandbox, &original, |_| {})
                .unwrap()
        });
        let second = scope.spawn(|| {
            b.prepare_execution_recipe(&sandbox, &original, |_| {})
                .unwrap()
        });
        let first = first.join().unwrap();
        assert_eq!(first.artifact, second.join().unwrap().artifact);
        first
    });
    assert_only_tag(image_id(&old), &canonical);
    // Simulate a legacy installation protected while another build replaces its recipe tag.
    let legacy = tags.tag(
        image_id(&old),
        format!("devdesk/prepared-codex:{}", &image_id(&old)[7..]),
    );
    let archive = tags.track(format!("{canonical}-{}", &image_id(&old)[7..]));
    let context = tempfile::tempdir().unwrap();
    std::fs::write(
        context.path().join("Dockerfile"),
        format!("FROM {base}\nLABEL devdesk.acceptance=rebuilt\n"),
    )
    .unwrap();
    let rebuilt = docker(&[
        "build",
        "--quiet",
        "--label",
        &format!("devdesk.recipe={}", original.recipe_digest()),
        "--tag",
        &canonical,
        context.path().to_str().unwrap(),
    ]);
    assert_ne!(image_id(&old), rebuilt);
    // A fresh dataset adopts the new artifact; existing ones keep their frozen Image ID.
    let third = tempfile::tempdir().unwrap();
    let (c, _) = setup(third.path());
    let new = c
        .prepare_execution_recipe(&sandbox, &original, |_| {})
        .unwrap();
    assert_eq!(image_id(&new), rebuilt);
    assert_eq!(
        a.prepare_execution_recipe(&sandbox, &original, |_| {})
            .unwrap()
            .artifact,
        old.artifact
    );
    assert_only_tag(image_id(&old), &archive);
    assert_only_tag(image_id(&new), &canonical);
    assert!(
        !std::process::Command::new("docker")
            .args(["image", "inspect", &legacy])
            .output()
            .unwrap()
            .status
            .success()
    );
    a.require_runtime_verification(&old).unwrap();
    c.require_runtime_verification(&new).unwrap();

    // A mismatched recipe tag must survive until its replacement has built successfully.
    let other = source(
        "replacement",
        &format!("FROM {base}\nLABEL devdesk.acceptance=replacement\n"),
    );
    let other_tag = tags.track(recipe_tag(&other));
    docker(&[
        "build",
        "--quiet",
        "--label",
        "devdesk.recipe=foreign",
        "--tag",
        &other_tag,
        context.path().to_str().unwrap(),
    ]);
    let displaced = docker(&["image", "inspect", "--format", "{{.Id}}", &other_tag]);
    let displaced_legacy = tags.tag(
        &displaced,
        format!("devdesk/prepared-codex:{}", &displaced[7..]),
    );
    let displaced_tag = tags.track(format!("{other_tag}-{}", &displaced[7..]));
    let new = b
        .prepare_execution_recipe(&sandbox, &other, |_| {})
        .unwrap();
    assert_ne!(image_id(&new), displaced);
    assert_only_tag(&displaced, &displaced_tag);
    assert_only_tag(image_id(&new), &other_tag);
    assert!(
        !std::process::Command::new("docker")
            .args(["image", "inspect", &displaced_legacy])
            .output()
            .unwrap()
            .status
            .success()
    );
    b.require_runtime_verification(&new).unwrap();
}
