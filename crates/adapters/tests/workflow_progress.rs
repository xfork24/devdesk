//! Read-model fixtures only. No workflow, model, Git or native worker is run.
use devdesk_adapters::{SqliteStore, performance};
use devdesk_core::EngineeringStore;
use devdesk_domain::{References, Scope, content_digest};
use rusqlite::{Connection, params};
use serde_json::json;
use std::sync::{Arc, Barrier};
const PROJECT: &str = "8e869077-8e14-4f67-acad-90152375190f";
fn fixture(root: &std::path::Path) -> (Arc<SqliteStore>, Connection) {
    let store = Arc::new(SqliteStore::open_project(root, PROJECT).unwrap());
    let conn =
        Connection::open(std::path::Path::new(&store.root()).join("engineering.sqlite3")).unwrap();
    let stages = (0..100)
        .map(|i| json!({"key":format!("stage-{i}"),"name":format!("Frozen stage {i}")}))
        .collect::<Vec<_>>();
    let definitions=(0..1000).map(|i|json!({"key":format!("step-{i}"),"stage":format!("stage-{}",i/10),"instructions":"x".repeat(100)})).collect::<Vec<_>>();
    for revision in [1, 2] {
        let doc = if revision == 1 {
            json!({"stages":stages,"steps":definitions})
        } else {
            json!({"stages":[{"key":"changed","name":"Newer defaults must not replace history"}],"steps":[]})
        };
        conn.execute("INSERT INTO source_definitions(identity,revision,type,name,scope,refs,data) VALUES('frozen-workflow',?1,'workflow','Frozen',?2,'{}',?3)",params![revision,serde_json::to_string(&Scope::project(PROJECT)).unwrap(),doc.to_string()]).unwrap();
    }
    let statuses = [
        "completed",
        "skipped",
        "running",
        "waiting_input",
        "waiting_confirmation",
        "awaiting_acceptance",
        "failed",
        "needs_reconciliation",
    ];
    let data = json!({"project_id":PROJECT,"owner_id":PROJECT,"root_instance_id":"root","status":"running","paused":false,"cancel_requested":false,"workflow_ref":"frozen-workflow:1","plan":{"$source":"workflow_plan","workflow_ref":"frozen-workflow:1","unrestorable_dependency":"missing"},"steps":(0..1000).map(|i|json!({"id":format!("execution-{i}"),"step":format!("step-{i}"),"status":statuses[i%8]})).collect::<Vec<_>>()});
    conn.execute("INSERT INTO governance_records(id,type,definition_id,scope,name,version,content_hash,refs,data,immutable,revision,created_at) VALUES('root','workflow_instance',?1,?2,'Root',1,?3,'{}',?4,0,7,1)",params![PROJECT,serde_json::to_string(&Scope::project(PROJECT)).unwrap(),content_digest(&References::new(),&data),data.to_string()]).unwrap();
    let bad =
        json!({"plan":{"$source":"missing-historical-source"},"history":"x".repeat(1_000_000)});
    conn.execute("INSERT INTO governance_records(id,type,definition_id,scope,name,version,content_hash,refs,data,immutable,revision,created_at) VALUES('unreadable-history','workflow_instance',?1,?2,'History',1,'fixture','{}',?3,0,1,1)",params![PROJECT,serde_json::to_string(&Scope::project(PROJECT)).unwrap(),bad.to_string()]).unwrap();
    (store, conn)
}
#[test]
fn stage_pages_use_exact_frozen_source_and_never_restore_or_compile_engineering_history() {
    let root = tempfile::tempdir().unwrap();
    let (store, conn) = fixture(root.path());
    let before = store.event_cursor().unwrap();
    let trace = performance::Trace::start();
    let mut offset = 0;
    let mut names = vec![];
    loop {
        let page = store.workflow_progress("root", offset).unwrap();
        assert_eq!(page.record.revision, 7);
        assert_eq!(page.counts.total, 1000);
        assert_eq!(page.counts.completed, 250);
        assert_eq!(page.counts.running, 125);
        assert_eq!(page.counts.waiting, 375);
        assert_eq!(page.counts.failed, 250);
        assert!(serde_json::to_vec(&page).unwrap().len() <= 64 * 1024);
        assert_eq!(page.stages.len(), 20);
        for stage in &page.stages {
            assert_eq!(stage.counts.total, 10);
            names.push(stage.name.clone());
        }
        match page.next_stage_offset {
            Some(next) => offset = next,
            None => break,
        }
    }
    assert_eq!(names.len(), 100);
    assert_eq!(names[0], "Frozen stage 0");
    assert_eq!(names[99], "Frozen stage 99");
    assert_eq!(store.event_cursor().unwrap(), before);
    let report = trace.finish();
    for stage in [
        "project.full_snapshot",
        "project.transaction",
        "source.restore_records",
        "source.restore_reference",
        "workflow.compile",
    ] {
        assert!(report["spans"][stage].is_null(), "{report}");
    }
    conn.execute("UPDATE governance_records SET data=json_set(data,'$.steps[0].status','running'),revision=8 WHERE id='root'",[]).unwrap();
    let next = store.workflow_progress("root", 0).unwrap();
    assert_eq!(next.record.revision, 8);
    assert_eq!(next.counts.completed, 249);
    assert_eq!(next.counts.running, 126);
}
#[test]
fn one_hundred_reads_see_committed_progress_while_sql_writer_holds_uncommitted_update() {
    let root = tempfile::tempdir().unwrap();
    let (store, mut conn) = fixture(root.path());
    let transaction = conn
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .unwrap();
    transaction.execute("UPDATE governance_records SET data=json_set(data,'$.status','cancelled'),revision=8 WHERE id='root'",[]).unwrap();
    let barrier = Arc::new(Barrier::new(100));
    let threads = (0..100)
        .map(|_| {
            let (store, barrier) = (store.clone(), barrier.clone());
            std::thread::spawn(move || {
                barrier.wait();
                store.workflow_progress("root", 0)
            })
        })
        .collect::<Vec<_>>();
    for thread in threads {
        let page = thread.join().unwrap().unwrap();
        assert_eq!(page.record.revision, 7);
        assert_eq!(page.record.data.status, "running");
    }
    transaction.commit().unwrap();
    assert_eq!(
        store
            .workflow_progress("root", 0)
            .unwrap()
            .record
            .data
            .status,
        "cancelled"
    );
}
#[test]
fn unknown_scope_and_missing_exact_source_do_not_fall_back_to_newer_defaults() {
    let root = tempfile::tempdir().unwrap();
    let (store, conn) = fixture(root.path());
    assert_eq!(
        store.workflow_progress("absent", 0).unwrap_err().code,
        "not_found"
    );
    assert!(store.workflow_progress("root", 1_000_001).is_err());
    conn.execute("UPDATE governance_records SET data=json_set(data,'$.plan.workflow_ref','frozen-workflow:3') WHERE id='root'",[]).unwrap();
    assert!(store.workflow_progress("root", 0).is_err());
    conn.execute(
        "UPDATE governance_records SET scope='\"project:other\"' WHERE id='root'",
        [],
    )
    .unwrap_err();
    conn.execute("INSERT INTO governance_records(id,type,definition_id,scope,name,version,content_hash,refs,data,immutable,revision,created_at) SELECT 'foreign',type,definition_id,'\"project:other\"',name,version,content_hash,refs,data,immutable,revision,created_at FROM governance_records WHERE id='root'",[]).unwrap();
    assert_eq!(
        store.workflow_progress("foreign", 0).unwrap_err().code,
        "not_found"
    );
}
