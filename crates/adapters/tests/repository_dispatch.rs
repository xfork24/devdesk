//! Dispatch/observation protocol only. No Git process, worker or external effect.
use devdesk_adapters::{SqliteStore, performance};
use devdesk_core::{
    Command, EngineeringStore, InvocationContext, concurrency::*,
    project_updates::ProjectUpdateContext, repository_dispatch::*,
    repository_updates::RepositoryRegistrationContext,
};
use devdesk_domain::{Project, Workspace};
use rusqlite::{Connection, params};
use serde_json::{Value, json};
use std::sync::{Arc, Barrier};
const PROJECT: &str = "bb8c1fd4-a972-4118-a710-1adfbe51bfc9";

fn fixture(root: &std::path::Path) -> (Arc<SqliteStore>, String, Connection) {
    let store = Arc::new(SqliteStore::open_project(root, PROJECT).unwrap());
    let project = Project {
        id: PROJECT.into(),
        title: "Dispatch fixture".into(),
        workspace: "/fixture/repository".into(),
        workspace_definition: Some(Workspace {
            repository: "fixture/repo".into(),
            path: "/fixture/repository".into(),
            mainline: "main".into(),
        }),
        connection: None,
        execution_mode: "live".into(),
        concurrency: 2,
        revision: 1,
    };
    let key = ResourceKey::new("project", PROJECT);
    store
        .commit_changes(&PreparedChangeSet {
            command_id: "fixture-project".into(),
            fingerprint: "fixture-project".into(),
            read_set: ReadSet::new(vec![ResourceVersion {
                resource: key.clone(),
                revision: 0,
            }])
            .unwrap(),
            changes: vec![ResourceChange {
                resource: key,
                value: json!(project),
            }],
            result: Value::Null,
            time: 1,
        })
        .unwrap();
    let command = Command {
        command_id: "register-fixture".into(),
        target: Some(PROJECT.into()),
        expected_revision: Some(1),
        payload: json!({"repository":"fixture/repo"}),
    };
    let prepared = RepositoryRegistrationContext::read(store.as_ref(), &command)
        .unwrap()
        .prepare(&command, &InvocationContext::human(Some(PROJECT.into())))
        .unwrap();
    let id = store.commit_changes(&prepared).unwrap().result["operation_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let connection = Connection::open(
        SqliteStore::project_path(root, PROJECT)
            .unwrap()
            .join("engineering.sqlite3"),
    )
    .unwrap();
    (store, id, connection)
}

#[test]
fn one_hundred_competing_attempts_admit_one_claim_with_original_actor() {
    let root = tempfile::tempdir().unwrap();
    let (store, id, connection) = fixture(root.path());
    let context = RepositoryDispatchContext::read(store.as_ref(), &id).unwrap();
    let prepared = (0..100)
        .map(|n| context.clone().prepare(&format!("attempt-{n}")).unwrap())
        .collect::<Vec<_>>();
    let barrier = Arc::new(Barrier::new(100));
    let threads = prepared
        .into_iter()
        .map(|prepared| {
            let (store, barrier) = (store.clone(), barrier.clone());
            std::thread::spawn(move || {
                barrier.wait();
                prepared.submit(store.as_ref())
            })
        })
        .collect::<Vec<_>>();
    let mut admitted = 0;
    for thread in threads {
        match thread.join().unwrap() {
            Ok(DispatchAdmission::Admitted(dispatch)) => {
                admitted += 1;
                assert_eq!(dispatch.operation().id, id);
                assert_eq!(dispatch.operation().revision, 1);
                assert_eq!(dispatch.mode(), DispatchMode::Execute);
            }
            Err(error) => assert_eq!(error.code, "revision_conflict"),
            Ok(DispatchAdmission::Replayed(_)) => panic!("distinct attempts cannot replay"),
        }
    }
    assert_eq!(admitted, 1);
    let (count, actor): (u64, String) = connection.query_row("SELECT count(*),json_extract(data,'$.actor.kind') FROM governance_records WHERE type='repository_dispatch'",[],|r|Ok((r.get(0)?,r.get(1)?))).unwrap();
    assert_eq!((count, actor.as_str()), (1, "human"));
    let (status, revision): (String, u64) = connection
        .query_row(
            "SELECT status,revision FROM repository_operations WHERE id=?1",
            [&id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!((status.as_str(), revision), ("dispatched", 2));
    assert!(RepositoryDispatchContext::read(store.as_ref(), &id).is_err());
}

#[test]
fn concurrent_replays_never_admit_another_external_attempt() {
    let root = tempfile::tempdir().unwrap();
    let (store, id, _) = fixture(root.path());
    let context = RepositoryDispatchContext::read(store.as_ref(), &id).unwrap();
    let barrier = Arc::new(Barrier::new(100));
    let threads = (0..100)
        .map(|_| {
            let (store, barrier) = (store.clone(), barrier.clone());
            let prepared = context.clone().prepare("same-original-attempt").unwrap();
            std::thread::spawn(move || {
                barrier.wait();
                prepared.submit(store.as_ref())
            })
        })
        .collect::<Vec<_>>();
    let mut admitted = 0;
    let mut replayed = 0;
    for thread in threads {
        match thread.join().unwrap().unwrap() {
            DispatchAdmission::Admitted(_) => admitted += 1,
            DispatchAdmission::Replayed(receipt) => {
                assert_eq!(receipt.result["operation_id"], id);
                replayed += 1;
            }
        }
    }
    assert_eq!((admitted, replayed), (1, 99));
}

#[test]
fn dispatch_rechecks_workspace_identity_and_its_original_project_read_version() {
    let root = tempfile::tempdir().unwrap();
    let (store, id, _) = fixture(root.path());
    let original = RepositoryDispatchContext::read(store.as_ref(), &id).unwrap();
    let update = |revision, payload| {
        let command = Command {
            command_id: format!("edit-project-{revision}"),
            target: Some(PROJECT.into()),
            expected_revision: Some(revision),
            payload,
        };
        let prepared = ProjectUpdateContext::read(store.as_ref(), &command)
            .unwrap()
            .prepare(&command, &InvocationContext::human(Some(PROJECT.into())))
            .unwrap();
        store.commit_changes(&prepared).unwrap();
    };
    update(1, json!({"title":"Concurrent title edit"}));
    assert!(matches!(
        original.prepare("stale-project").unwrap().submit(store.as_ref()),
        Err(error) if error.code == "revision_conflict"
    ));
    // A title changed before context acquisition does not invalidate the frozen
    // workspace. Changing the actual workspace must fence the old Git intent.
    let after_title = RepositoryDispatchContext::read(store.as_ref(), &id).unwrap();
    update(2, json!({"workspace":"/different/repository"}));
    assert!(matches!(
        after_title.prepare("stale-workspace").unwrap().submit(store.as_ref()),
        Err(error) if error.code == "revision_conflict"
    ));
    assert!(matches!(
        RepositoryDispatchContext::read(store.as_ref(), &id),
        Err(error) if error.code == "revision_conflict"
    ));
}

#[test]
fn a_late_unknown_result_is_preserved_without_overwriting_newer_operation_state() {
    let root = tempfile::tempdir().unwrap();
    let (store, id, connection) = fixture(root.path());
    let context = RepositoryDispatchContext::read(store.as_ref(), &id).unwrap();
    let dispatch = match context
        .prepare("original-attempt")
        .unwrap()
        .submit(store.as_ref())
        .unwrap()
    {
        DispatchAdmission::Admitted(dispatch) => dispatch,
        _ => panic!("expected a new claim"),
    };
    // Simulated restart/control fencing. This fixture does not assert that the
    // old production callback has already migrated to the new protocol.
    connection.execute("UPDATE repository_operations SET status='needs_reconciliation',revision=revision+1,reason='restart_after_dispatch' WHERE id=?1",[&id]).unwrap();
    let observation = RepositoryObservation::Failed {
        code: "transport_lost".into(),
        message: "Test-only unknown external outcome".into(),
    };
    let claim_id = dispatch.claim_id().to_owned();
    drop(dispatch);
    let observer = RepositoryObservationContext::read(store.as_ref(), &claim_id).unwrap();
    let receipt = observer.record(store.as_ref(), &observation).unwrap();
    let replay = observer.record(store.as_ref(), &observation).unwrap();
    assert_eq!(receipt.cursor, replay.cursor);
    assert_eq!(
        observer
            .record(
                store.as_ref(),
                &RepositoryObservation::Failed {
                    code: "different-outcome".into(),
                    message: String::new(),
                },
            )
            .unwrap_err()
            .code,
        "invalid_params"
    );
    let (status, revision): (String, u64) = connection
        .query_row(
            "SELECT status,revision FROM repository_operations WHERE id=?1",
            [&id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!((status.as_str(), revision), ("needs_reconciliation", 3));
    let count: u64 = connection.query_row("SELECT count(*) FROM governance_records WHERE type='repository_observation' AND json_extract(data,'$.observation.code')='transport_lost'",[],|r|r.get(0)).unwrap();
    assert_eq!(count, 1);
    assert!(
        connection
            .execute(
                "UPDATE governance_records SET data='{}' WHERE type='repository_observation'",
                []
            )
            .is_err()
    );
}

#[test]
fn finite_dispatch_ignores_unrelated_history_and_never_guesses_legacy_actor() {
    let root = tempfile::tempdir().unwrap();
    let (store, id, connection) = fixture(root.path());
    connection.execute("INSERT INTO governance_records(id,type,definition_id,scope,name,version,content_hash,refs,data,immutable,revision,created_at) VALUES('unrestorable','workflow_instance','owner',?1,'history',1,'fixture','{}',?2,0,1,1)",params![json!(format!("project:{PROJECT}")).to_string(),json!({"root_instance_id":"unrestorable-root","plan":{"$source":"missing-input"},"large":"x".repeat(1_000_000)}).to_string()]).unwrap();
    let context = RepositoryDispatchContext::read(store.as_ref(), &id).unwrap();
    let other_id = "8bce7c46-e9f2-4079-99fb-90b061ef7df2";
    let other = SqliteStore::open_project(root.path(), other_id).unwrap();
    assert!(matches!(
        context.clone().prepare("wrong-scope").unwrap().submit(&other),
        Err(error) if error.code == "invalid_scope"
    ));
    assert!(RepositoryDispatchContext::read(&other, &id).is_err());
    let trace = performance::Trace::start();
    let dispatch = RepositoryDispatchContext::read(store.as_ref(), &id)
        .unwrap()
        .prepare("finite-attempt")
        .unwrap()
        .submit(store.as_ref())
        .unwrap();
    assert!(matches!(dispatch, DispatchAdmission::Admitted(_)));
    let report = trace.finish();
    for span in [
        "project.full_snapshot",
        "project.transaction",
        "source.restore_records",
        "workflow.compile",
    ] {
        assert!(report["spans"][span].is_null(), "{report}");
    }
    connection
        .execute(
            "UPDATE repository_operations SET status='completed',revision=revision+1 WHERE id=?1",
            [&id],
        )
        .unwrap();
    connection.execute("INSERT INTO repository_operations(id,project_id,action,status,receipt,reason,revision) VALUES('legacy-operation',?1,?2,'prepared',NULL,'fixture without provenance',1)",params![PROJECT,json!({"kind":"create_worktree","owner":"legacy-owner","worktree_kind":"feature","base":"a".repeat(40)}).to_string()]).unwrap();
    assert!(matches!(
        RepositoryDispatchContext::read(store.as_ref(), "legacy-operation"),
        Err(error) if error.code == "provenance_required"
    ));
}

#[test]
fn observation_recovery_rejects_missing_or_overflowed_dispatch_versions() {
    let root = tempfile::tempdir().unwrap();
    let (store, id, connection) = fixture(root.path());
    let dispatch = match RepositoryDispatchContext::read(store.as_ref(), &id)
        .unwrap()
        .prepare("version-fault-fixture")
        .unwrap()
        .submit(store.as_ref())
        .unwrap()
    {
        DispatchAdmission::Admitted(dispatch) => dispatch,
        _ => panic!("expected new dispatch"),
    };
    let original = store
        .read_resources(&[ResourceKey::new("governance", dispatch.claim_id())])
        .unwrap()
        .remove(0)
        .value
        .unwrap();
    for (index, expected, dispatched) in [
        (0, json!(u64::MAX), Value::Null),
        (1, json!(0), json!(1)),
        (2, json!(1), Value::Null),
    ] {
        let mut row = devdesk_domain::ConfigurationRecord::from_storage(original.clone()).unwrap();
        row.id = format!("malformed-version-{index}");
        row.data["expected_operation_revision"] = expected;
        row.data["dispatched_operation_revision"] = dispatched;
        row.content_hash = devdesk_domain::content_digest(&row.refs, &row.data);
        // A well-hashed but structurally invalid journal is a corruption fixture,
        // not an external receipt or a production dispatch admission.
        connection.execute("INSERT INTO governance_records(id,type,definition_id,scope,name,version,content_hash,refs,data,immutable,revision,created_at) VALUES(?1,'repository_dispatch',?2,?3,'version fault',1,?4,'{}',?5,1,1,1)",params![row.id,row.definition_id,json!(row.scope).to_string(),row.content_hash,row.data.to_string()]).unwrap();
        assert!(matches!(
            RepositoryObservationContext::read(store.as_ref(), &row.id),
            Err(error) if error.code == "storage"
        ));
    }
}

#[test]
fn completed_git_activity_after_intent_creation_still_fences_first_dispatch() {
    let root = tempfile::tempdir().unwrap();
    let (store, id, connection) = fixture(root.path());
    connection.execute("INSERT INTO governance_records(id,type,definition_id,scope,name,version,content_hash,refs,data,immutable,revision,created_at) VALUES('intervening-git','platform_call','other',?1,'Git',1,'fixture','{}',?2,0,1,1)",params![json!(format!("project:{PROJECT}")).to_string(),json!({"status":"completed","request":{"definition":{"qualified_name":"git.status"}}}).to_string()]).unwrap();
    assert!(
        matches!(RepositoryDispatchContext::read(store.as_ref(),&id),Err(error) if error.code=="revision_conflict")
    );
    let (status, revision): (String, u64) = connection
        .query_row(
            "SELECT status,revision FROM repository_operations WHERE id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!((status.as_str(), revision), ("prepared", 1));
}
