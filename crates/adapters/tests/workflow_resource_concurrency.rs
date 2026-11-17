//! Deterministic resource/reader acceptance. No production dataset or supplier.
use devdesk_adapters::{SqliteStore, performance};
use devdesk_core::{EngineeringStore, concurrency::*};
use devdesk_domain::*;
use serde_json::json;
use std::sync::{Arc, Barrier};
fn version(row: &ResourceSnapshot) -> ResourceVersion {
    ResourceVersion {
        resource: row.resource.clone(),
        revision: row.revision,
    }
}
fn change(
    command: &str,
    reads: Vec<ResourceVersion>,
    changes: Vec<ResourceChange>,
) -> PreparedChangeSet {
    PreparedChangeSet {
        command_id: command.into(),
        fingerprint: configuration_hash(&(command, &reads, &changes)),
        read_set: ReadSet::new(reads).unwrap(),
        changes,
        result: json!({"id":command}),
        time: 1,
    }
}
fn fixture(root: &std::path::Path) -> Arc<SqliteStore> {
    let project = "985a06d7-209d-4a19-b5d9-f8bd9d711617";
    let store = Arc::new(SqliteStore::open_project(root, project).unwrap());
    let source = SourceDefinition {
        identity: "concurrency-workflow".into(),
        revision: 1,
        r#type: "workflow".into(),
        scope: Scope::project(project),
        name: "Concurrency fixture".into(),
        refs: References::new(),
        data: json!({"stages":[{"key":"stage","name":"Stage"}],"steps":[{"key":"a","name":"A","stage":"stage"},{"key":"b","name":"B","stage":"stage"}]}),
    };
    let key = ResourceKey::new("source", "concurrency-workflow:1");
    let head = ResourceKey::new("source_head", "concurrency-workflow");
    store
        .commit_changes(&change(
            "source",
            vec![
                ResourceVersion {
                    resource: key.clone(),
                    revision: 0,
                },
                ResourceVersion {
                    resource: head.clone(),
                    revision: 0,
                },
            ],
            vec![
                ResourceChange {
                    resource: key,
                    value: json!(source),
                },
                ResourceChange {
                    resource: head,
                    value: json!({"reference":"concurrency-workflow:1"}),
                },
            ],
        ))
        .unwrap();
    let data = json!({"project_id":project,"owner_id":project,"root_instance_id":"root","status":"running","paused":false,"cancel_requested":false,"workflow_ref":"concurrency-workflow:1","control_version":1,"active_plan_revision_id":"plan","plan":{"$source":"workflow_plan","workflow_ref":"concurrency-workflow:1"},"steps":[{"id":"a","workflow_instance_id":"root","step":"a","node_id":"a","status":"running","outputs":{}},{"id":"b","workflow_instance_id":"root","step":"b","node_id":"b","status":"running","outputs":{}}]});
    let row = ConfigurationRecord {
        id: "root".into(),
        r#type: "workflow_instance".into(),
        definition_id: project.into(),
        scope: Scope::project(project),
        name: "Root".into(),
        version: 1,
        revision: 1,
        created_at: 1,
        supersedes: None,
        immutable: false,
        refs: References::new(),
        content_hash: content_digest(&References::new(), &data),
        data,
    };
    let key = ResourceKey::new("governance", "root");
    store
        .commit_changes(&change(
            "root",
            vec![ResourceVersion {
                resource: key.clone(),
                revision: 0,
            }],
            vec![ResourceChange {
                resource: key,
                value: row.to_storage(),
            }],
        ))
        .unwrap();
    store
}
fn step(store: &SqliteStore, id: &str) -> ResourceSnapshot {
    store
        .read_resources(&[ResourceKey::new(
            "workflow_step",
            json!(["root", id]).to_string(),
        )])
        .unwrap()
        .remove(0)
}
fn advance(
    store: &SqliteStore,
    id: &str,
    command: &str,
    status: &str,
) -> devdesk_domain::Result<CommitReceipt> {
    let row = step(store, id);
    let mut value = row.value.clone().unwrap();
    value["status"] = json!(status);
    store.commit_changes(&change(
        command,
        vec![version(&row)],
        vec![ResourceChange {
            resource: row.resource,
            value,
        }],
    ))
}
#[test]
fn batched_step_reads_preserve_order_absence_and_native_versions_after_structural_changes() {
    let directory = tempfile::tempdir().unwrap();
    let store = fixture(directory.path());
    let keys = [
        ResourceKey::new("workflow_step", json!(["root", "b"]).to_string()),
        ResourceKey::new("workflow_step", json!(["root", "absent"]).to_string()),
        ResourceKey::new("governance", "root"),
        ResourceKey::new("workflow_step", json!(["root", "a"]).to_string()),
        ResourceKey::new("workflow_step", json!(["root", "b"]).to_string()),
        ResourceKey::new("workflow_step", json!(["absent", "a"]).to_string()),
    ];
    let compare = || {
        let together = store.read_resources(&keys).unwrap();
        for (key, row) in keys.iter().zip(&together) {
            let separate = store.read_resources(std::slice::from_ref(key)).unwrap();
            assert_eq!(json!(row), json!(separate[0]));
        }
        together
    };
    let original = compare();
    let connection =
        rusqlite::Connection::open(std::path::Path::new(&store.root()).join("engineering.sqlite3"))
            .unwrap();
    // Isolated SQL trigger fixture: reordering is not a semantic step update.
    connection.execute("UPDATE governance_records SET data=json_set(data,'$.steps',json_array(json_extract(data,'$.steps[1]'),json_extract(data,'$.steps[0]'))),revision=revision+1 WHERE id='root'",[]).unwrap();
    let reordered = compare();
    assert_eq!(original[0].revision, reordered[0].revision);
    assert_eq!(original[3].revision, reordered[3].revision);
    connection.execute("UPDATE governance_records SET data=json_set(data,'$.steps[0].id','replacement'),revision=revision+1 WHERE id='root'",[]).unwrap();
    let replaced = compare();
    assert!(replaced[0].value.is_none());
    assert!(replaced[0].revision > original[0].revision);
    connection.execute("UPDATE governance_records SET data=json_remove(data,'$.steps[1]'),revision=revision+1 WHERE id='root'",[]).unwrap();
    let removed = compare();
    assert!(removed[3].value.is_none());
    assert!(removed[3].revision > original[3].revision);
}

#[test]
fn sibling_steps_commit_from_the_same_header_without_lost_updates_and_stale_target_conflicts() {
    let root = tempfile::tempdir().unwrap();
    let store = fixture(root.path());
    let header = store
        .read_resources(&[ResourceKey::new("workflow_header", "root")])
        .unwrap()
        .remove(0);
    let rows = [step(&store, "a"), step(&store, "b")];
    let old = rows[0].clone();
    let gate = Arc::new(Barrier::new(2));
    let jobs = rows
        .into_iter()
        .enumerate()
        .map(|(i, row)| {
            let store = store.clone();
            let gate = gate.clone();
            let header = version(&header);
            std::thread::spawn(move || {
                let mut value = row.value.clone().unwrap();
                value["status"] = json!("completed");
                gate.wait();
                store.commit_changes(&change(
                    &format!("sibling-{i}"),
                    vec![version(&row), header],
                    vec![ResourceChange {
                        resource: row.resource,
                        value,
                    }],
                ))
            })
        })
        .collect::<Vec<_>>();
    for job in jobs {
        job.join().unwrap().unwrap();
    }
    assert_eq!(step(&store, "a").value.unwrap()["status"], "completed");
    assert_eq!(step(&store, "b").value.unwrap()["status"], "completed");
    let stale = change(
        "late-original-callback",
        vec![version(&old)],
        vec![ResourceChange {
            resource: old.resource.clone(),
            value: old.value.unwrap(),
        }],
    );
    assert_eq!(
        store.commit_changes(&stale).unwrap_err().code,
        "revision_conflict"
    );
    assert_eq!(
        store.workflow_progress("root", 0).unwrap().counts.completed,
        2
    );
    let conn =
        rusqlite::Connection::open(std::path::Path::new(&store.root()).join("engineering.sqlite3"))
            .unwrap();
    let complete:u64=conn.query_row("SELECT count(*) FROM workflow_display_steps WHERE json_extract(data,'$.status')='completed'",[],|r|r.get(0)).unwrap();
    assert_eq!(complete, 2);
}
#[test]
fn one_hundred_readers_continue_while_resource_progress_is_committed() {
    let root = tempfile::tempdir().unwrap();
    let store = fixture(root.path());
    let barrier = Arc::new(Barrier::new(101));
    let jobs = (0..100)
        .map(|_| {
            let store = store.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let trace = performance::Trace::start();
                barrier.wait();
                for _ in 0..20 {
                    let progress = store.workflow_progress("root", 0).unwrap();
                    assert_eq!(progress.counts.total, 2);
                    let row = step(&store, "a");
                    assert!(row.revision >= 1);
                }
                let report = trace.finish();
                for stage in [
                    "project.full_snapshot",
                    "project.transaction",
                    "workflow.compile",
                ] {
                    assert!(report["spans"][stage].is_null(), "{report}");
                }
            })
        })
        .collect::<Vec<_>>();
    barrier.wait();
    for i in 0..20 {
        advance(
            &store,
            "a",
            &format!("progress-{i}"),
            if i % 2 == 0 { "completed" } else { "running" },
        )
        .unwrap();
    }
    for job in jobs {
        job.join().unwrap();
    }
    assert_eq!(step(&store, "a").revision, 21);
}

#[test]
fn selected_identity_change_conflicts_even_when_successor_has_the_same_revision() {
    let root = tempfile::tempdir().unwrap();
    let store = fixture(root.path());
    let make = |id: &str, status: &str| {
        let data = json!({"owner_id":"owner","workspace_only":false,"status":status});
        ConfigurationRecord {
            id: id.into(),
            r#type: "workflow_controller".into(),
            definition_id: "owner".into(),
            scope: Scope::project("985a06d7-209d-4a19-b5d9-f8bd9d711617"),
            name: "Controller fixture".into(),
            version: 1,
            revision: 1,
            created_at: 1,
            supersedes: None,
            immutable: false,
            refs: References::new(),
            content_hash: content_digest(&References::new(), &data),
            data,
        }
    };
    let first = ResourceKey::new("governance", "controller-old");
    store
        .commit_changes(&change(
            "controller-old",
            vec![ResourceVersion {
                resource: first.clone(),
                revision: 0,
            }],
            vec![ResourceChange {
                resource: first.clone(),
                value: make("controller-old", "active").to_storage(),
            }],
        ))
        .unwrap();
    let alias = ResourceKey::new(
        "controller_owner_source",
        json!(["owner", false]).to_string(),
    );
    let selected = store
        .read_resources(std::slice::from_ref(&alias))
        .unwrap()
        .remove(0);
    assert_eq!(selected.revision, 1);
    let witness = ResourceKey::new(
        "selected_record",
        json!([alias.kind, alias.id, "controller-old"]).to_string(),
    );
    let mut closed = make("controller-old", "closed");
    closed.revision = 2;
    let next = ResourceKey::new("governance", "controller-new");
    store
        .commit_changes(&change(
            "replace-controller",
            vec![
                ResourceVersion {
                    resource: first.clone(),
                    revision: 1,
                },
                ResourceVersion {
                    resource: next.clone(),
                    revision: 0,
                },
            ],
            vec![
                ResourceChange {
                    resource: first,
                    value: closed.to_storage(),
                },
                ResourceChange {
                    resource: next,
                    value: make("controller-new", "active").to_storage(),
                },
            ],
        ))
        .unwrap();
    let now = store.read_resources(&[alias]).unwrap().remove(0);
    assert_eq!(now.revision, 1);
    assert_ne!(selected.value.unwrap()["id"], now.value.unwrap()["id"]);
    let target = ResourceKey::new("governance", "late-observation");
    let old = change(
        "late-from-old-controller",
        vec![
            ResourceVersion {
                resource: witness,
                revision: 1,
            },
            ResourceVersion {
                resource: target.clone(),
                revision: 0,
            },
        ],
        vec![ResourceChange {
            resource: target.clone(),
            value: make("late-observation", "active").to_storage(),
        }],
    );
    assert_eq!(
        store.commit_changes(&old).unwrap_err().code,
        "revision_conflict"
    );
    assert!(store.read_resources(&[target]).unwrap()[0].value.is_none());
}
