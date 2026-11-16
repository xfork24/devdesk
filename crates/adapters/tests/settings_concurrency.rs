use devdesk_adapters::{GlobalStore, SqliteStore, performance};
use devdesk_core::{
    Command, EngineeringStore, InvocationContext,
    concurrency::{ReadSet, ResourceVersion},
    settings,
};
use serde_json::{Value, json};

fn save(
    store: &dyn EngineeringStore,
    baseline: &[ResourceVersion],
    id: &str,
    payload: Value,
) -> devdesk_domain::Result<devdesk_core::concurrency::CommitReceipt> {
    let guards = baseline
        .iter()
        .filter(|v| payload.get(&v.resource.id).is_some())
        .cloned()
        .collect();
    settings::save(
        store,
        &Command {
            command_id: id.into(),
            target: None,
            expected_revision: None,
            payload,
        },
        &ReadSet::new(guards)?,
        &InvocationContext::human(None),
    )
}

#[test]
fn independent_edits_commit_but_stale_same_field_cannot_overwrite() {
    let directory = tempfile::tempdir().unwrap();
    let store = GlobalStore::open(directory.path()).unwrap();
    let baseline = settings::read(&store).unwrap().versions;
    let first = save(
        &store,
        &baseline,
        "concurrency",
        json!({"global_concurrency":7}),
    )
    .unwrap();
    save(
        &store,
        &baseline,
        "provider",
        json!({"default_harness":"dsh"}),
    )
    .unwrap();
    let conflict = save(&store, &baseline, "stale", json!({"global_concurrency":2})).unwrap_err();
    assert_eq!(conflict.code, "revision_conflict");
    let current = settings::read(&store).unwrap();
    assert_eq!(current.global_concurrency, 7);
    assert_eq!(
        current.default_harness,
        devdesk_domain::HarnessProvider::Dsh
    );
    save(
        &store,
        &current.versions,
        "newer",
        json!({"global_concurrency":8}),
    )
    .unwrap();
    let replay = save(
        &store,
        &baseline,
        "concurrency",
        json!({"global_concurrency":7}),
    )
    .unwrap();
    assert_eq!(replay.cursor, first.cursor);
    assert_eq!(replay.versions, first.versions);
    assert_eq!(replay.result, first.result);
    assert_eq!(settings::read(&store).unwrap().global_concurrency, 8);
    assert!(
        save(
            &store,
            &baseline,
            "concurrency",
            json!({"global_concurrency":9})
        )
        .is_err()
    );
    assert_eq!(store.events_after(0, 100).unwrap().len(), 3);
}

#[test]
fn invalid_batches_and_missing_guards_leave_both_settings_unchanged() {
    let directory = tempfile::tempdir().unwrap();
    let store = GlobalStore::open(directory.path()).unwrap();
    let baseline = settings::read(&store).unwrap();
    for (i, payload) in [
        json!({}),
        json!({"default_harness":"dsh","global_concurrency":0}),
        json!({"default_harness":null}),
        json!({"global_concurrency":33}),
        json!({"unknown":1}),
        json!({"global_concurrency":1.5}),
    ]
    .into_iter()
    .enumerate()
    {
        assert!(save(&store, &baseline.versions, &format!("invalid-{i}"), payload).is_err());
    }
    let missing = save(&store, &[], "missing", json!({"default_harness":"dsh"})).unwrap_err();
    assert_eq!(missing.code, "precondition_required");
    let command = Command {
        command_id: "extra".into(),
        target: None,
        expected_revision: None,
        payload: json!({"default_harness":"dsh"}),
    };
    assert!(
        settings::save(
            &store,
            &command,
            &ReadSet::new(baseline.versions.clone()).unwrap(),
            &InvocationContext::human(None)
        )
        .is_err()
    );
    assert_eq!(
        serde_json::to_value(settings::read(&store).unwrap()).unwrap(),
        serde_json::to_value(baseline).unwrap()
    );
    assert!(store.events_after(0, 100).unwrap().is_empty());
}

#[test]
fn configuration_namespace_is_explicit_and_reads_never_restore_history() {
    let directory = tempfile::tempdir().unwrap();
    let store = GlobalStore::open(&directory.path().join("global")).unwrap();
    let trace = performance::Trace::start();
    settings::read(&store).unwrap();
    assert!(trace.finish()["spans"]["global.full_snapshot"].is_null());
    let engineering = SqliteStore::open_project(
        &directory.path().join("project"),
        "c8516249-60a3-444a-988d-e5bf0e257611",
    )
    .unwrap();
    assert_eq!(
        settings::read(&engineering).unwrap_err().code,
        "invalid_scope"
    );
    assert_eq!(
        save(
            &engineering,
            &[],
            "wrong-namespace",
            json!({"global_concurrency":7})
        )
        .unwrap_err()
        .code,
        "invalid_scope"
    );
}
