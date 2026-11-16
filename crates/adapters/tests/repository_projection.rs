//! Trusted-port receipt fixtures exercise atomic admission, not actual Git/GitHub.
use devdesk_adapters::{SqliteStore, performance};
use devdesk_core::{
    Command, EngineeringStore, InvocationContext, concurrency::*,
    project_updates::ProjectUpdateContext, repository_dispatch::*,
    repository_projection::accept_connect_observation,
    repository_updates::RepositoryRegistrationContext,
};
use devdesk_domain::*;
use rusqlite::{Connection, params};
use serde_json::{Value, json};
use std::sync::{Arc, Barrier};
const PROJECT: &str = "18aed273-f831-458b-b368-f759eecc5ae3";
struct Fixture {
    store: Arc<SqliteStore>,
    dispatch: Box<RepositoryDispatch>,
}
impl Fixture {
    fn new(root: &std::path::Path) -> Self {
        let store = Arc::new(SqliteStore::open_project(root, PROJECT).unwrap());
        let project = Project {
            id: PROJECT.into(),
            title: "Before registration".into(),
            workspace: "/unattached".into(),
            workspace_definition: None,
            execution_mode: "live".into(),
            concurrency: 2,
            revision: 1,
        };
        let resource = ResourceKey::new("project", PROJECT);
        store
            .commit_changes(&PreparedChangeSet {
                command_id: "fixture-project".into(),
                fingerprint: "fixture-project".into(),
                read_set: ReadSet::new(vec![ResourceVersion {
                    resource: resource.clone(),
                    revision: 0,
                }])
                .unwrap(),
                changes: vec![ResourceChange {
                    resource,
                    value: json!(project),
                }],
                result: Value::Null,
                time: 1,
            })
            .unwrap();
        let command = Command {
            command_id: "fixture-registration".into(),
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
        let dispatch = match RepositoryDispatchContext::read(store.as_ref(), &id)
            .unwrap()
            .prepare("fixture-attempt")
            .unwrap()
            .submit(store.as_ref())
            .unwrap()
        {
            DispatchAdmission::Admitted(dispatch) => dispatch,
            _ => panic!("expected new claim"),
        };
        Self { store, dispatch }
    }
    fn connected(&self) -> RepositoryReceipt {
        let head = "a".repeat(40);
        RepositoryReceipt::Connected {
            policy: GitHubRepositoryPolicy {
                repository: "fixture/repo".into(),
                repository_id: "fixture-repository-id".into(),
                branch: "main".into(),
                head: Some(head.clone()),
                viewer_permission: "write".into(),
                compatible: true,
                blocked_reasons: vec![],
                active_branch_rulesets: vec![],
                classic_protection_count: 0,
                observed_at: 1,
            },
            workspace: Workspace {
                repository: "fixture/repo".into(),
                path: format!("{}/repository", self.store.root()),
                mainline: "main".into(),
            },
            head: head.clone(),
            worktree: RepositoryWorktree {
                id: PROJECT.into(),
                project_id: PROJECT.into(),
                owner_id: PROJECT.into(),
                kind: WorktreeKind::Mainline,
                branch: "main".into(),
                path: format!(
                    "{}/execution/worktrees/mainlines/{PROJECT}",
                    self.store.root()
                ),
                base: head,
                operation_id: self.dispatch.operation().id.clone(),
            },
        }
    }
    fn observe(&self, receipt: RepositoryReceipt) -> String {
        self.dispatch
            .record_observation(
                self.store.as_ref(),
                &RepositoryObservation::Received {
                    receipt: Box::new(receipt),
                },
            )
            .unwrap()
            .result["observation_id"]
            .as_str()
            .unwrap()
            .to_owned()
    }
    fn edit(&self, revision: u64, payload: Value) {
        let command = Command {
            command_id: format!("edit-{revision}"),
            target: Some(PROJECT.into()),
            expected_revision: Some(revision),
            payload,
        };
        let prepared = ProjectUpdateContext::read(self.store.as_ref(), &command)
            .unwrap()
            .prepare(&command, &InvocationContext::human(Some(PROJECT.into())))
            .unwrap();
        self.store.commit_changes(&prepared).unwrap();
    }
    fn connection(&self) -> Connection {
        Connection::open(std::path::Path::new(&self.store.root()).join("engineering.sqlite3"))
            .unwrap()
    }
    fn project(&self) -> Value {
        self.store
            .read_resources(&[ResourceKey::new("project", PROJECT)])
            .unwrap()
            .remove(0)
            .value
            .unwrap()
    }
    fn assert_unaccepted(&self, observation: &str) {
        let rows = self
            .store
            .read_resources(&[
                ResourceKey::new("repository_operation", &self.dispatch.operation().id),
                ResourceKey::new("repository_worktree", PROJECT),
                ResourceKey::new("head", format!("main:{PROJECT}")),
                ResourceKey::new("governance", observation),
            ])
            .unwrap();
        assert_eq!(rows[0].value.as_ref().unwrap()["status"], "dispatched");
        assert_eq!(rows[0].value.as_ref().unwrap()["revision"], 2);
        assert!(rows[1].value.is_none() && rows[2].value.is_none());
        assert!(rows[3].value.is_some());
    }
}

#[test]
fn one_hundred_observation_acceptances_publish_one_atomic_result_and_replay_original() {
    let root = tempfile::tempdir().unwrap();
    let fixture = Fixture::new(root.path());
    let observation = fixture.observe(fixture.connected());
    let barrier = Arc::new(Barrier::new(100));
    let threads = (0..100)
        .map(|_| {
            let (store, observation, barrier) =
                (fixture.store.clone(), observation.clone(), barrier.clone());
            std::thread::spawn(move || {
                barrier.wait();
                accept_connect_observation(store.as_ref(), &observation)
            })
        })
        .collect::<Vec<_>>();
    let mut cursor = None;
    for thread in threads {
        let receipt = thread.join().unwrap().unwrap();
        assert_eq!(receipt.result["status"], "completed");
        if let Some(previous) = cursor {
            assert_eq!(receipt.cursor, previous);
        } else {
            cursor = Some(receipt.cursor);
        }
    }
    let connection = fixture.connection();
    let (status, revision): (String, u64) = connection
        .query_row(
            "SELECT status,revision FROM repository_operations",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!((status.as_str(), revision), ("completed", 3));
    let (worktrees,heads,acceptances):(u64,u64,u64)=connection.query_row("SELECT (SELECT count(*) FROM repository_worktrees),(SELECT count(*) FROM heads),(SELECT count(*) FROM governance_records WHERE type='repository_acceptance')",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap();
    assert_eq!((worktrees, heads, acceptances), (1, 1, 1));
    fixture.edit(2, json!({"title":"Later title"}));
    assert_eq!(
        accept_connect_observation(fixture.store.as_ref(), &observation)
            .unwrap()
            .cursor,
        cursor.unwrap()
    );
    assert_eq!(fixture.project()["title"], "Later title");
}

#[test]
fn owned_workspace_projection_retains_concurrent_title_and_quota_edits_without_history_restore() {
    let root = tempfile::tempdir().unwrap();
    let fixture = Fixture::new(root.path());
    let observation = fixture.observe(fixture.connected());
    fixture.edit(
        1,
        json!({"title":"Edited while external port ran","concurrency":8}),
    );
    let connection = fixture.connection();
    connection.execute("INSERT INTO governance_records(id,type,definition_id,scope,name,version,content_hash,refs,data,immutable,revision,created_at) VALUES('history','workflow_instance','owner',?1,'unrestorable',1,'fixture','{}',?2,0,1,1)",params![json!(format!("project:{PROJECT}")).to_string(),json!({"plan":{"$source":"missing"},"large":"x".repeat(1_000_000)}).to_string()]).unwrap();
    let trace = performance::Trace::start();
    accept_connect_observation(fixture.store.as_ref(), &observation).unwrap();
    let report = trace.finish();
    for span in [
        "project.full_snapshot",
        "project.transaction",
        "source.restore_records",
        "workflow.compile",
    ] {
        assert!(report["spans"][span].is_null(), "{report}");
    }
    let project = fixture.project();
    assert_eq!(project["title"], "Edited while external port ran");
    assert_eq!(project["concurrency"], 8);
    assert_eq!(project["revision"], 3);
    assert_eq!(
        project["workspace"],
        format!("{}/repository", fixture.store.root())
    );
}

#[test]
fn workspace_changes_and_intervening_completed_git_activity_fence_original_receipts() {
    for workspace_change in [true, false] {
        let root = tempfile::tempdir().unwrap();
        let fixture = Fixture::new(root.path());
        let observation = fixture.observe(fixture.connected());
        if workspace_change {
            fixture.edit(1, json!({"workspace":"/different-workspace"}));
        } else {
            fixture.connection().execute("INSERT INTO governance_records(id,type,definition_id,scope,name,version,content_hash,refs,data,immutable,revision,created_at) VALUES('other-git','platform_call','other',?1,'Git',1,'fixture','{}',?2,0,1,1)",params![json!(format!("project:{PROJECT}")).to_string(),json!({"status":"completed","request":{"definition":{"qualified_name":"git.status"}}}).to_string()]).unwrap();
        }
        assert_eq!(
            accept_connect_observation(fixture.store.as_ref(), &observation)
                .unwrap_err()
                .code,
            "revision_conflict"
        );
        fixture.assert_unaccepted(&observation);
    }
}

#[test]
fn wrong_worktree_receipts_are_retained_but_never_projected() {
    let root = tempfile::tempdir().unwrap();
    let fixture = Fixture::new(root.path());
    let mut receipt = fixture.connected();
    if let RepositoryReceipt::Connected { worktree, .. } = &mut receipt {
        worktree.path = "/foreign/worktree".into();
    }
    let observation = fixture.observe(receipt);
    assert_eq!(
        accept_connect_observation(fixture.store.as_ref(), &observation)
            .unwrap_err()
            .code,
        "adapter_contract"
    );
    fixture.assert_unaccepted(&observation);
    assert_eq!(fixture.project()["workspace"], "/unattached");
}

#[test]
fn unknown_failure_admission_marks_only_the_original_operation_for_reconciliation() {
    let root = tempfile::tempdir().unwrap();
    let fixture = Fixture::new(root.path());
    let observation = fixture
        .dispatch
        .record_observation(
            fixture.store.as_ref(),
            &RepositoryObservation::Failed {
                code: "transport_lost".into(),
                message: "test unknown result".into(),
            },
        )
        .unwrap()
        .result["observation_id"]
        .as_str()
        .unwrap()
        .to_owned();
    fixture.edit(1, json!({"workspace":"/changed-after-unknown-effect"}));
    let first = accept_connect_observation(fixture.store.as_ref(), &observation).unwrap();
    assert_eq!(first.result["status"], "needs_reconciliation");
    assert_eq!(
        accept_connect_observation(fixture.store.as_ref(), &observation)
            .unwrap()
            .cursor,
        first.cursor
    );
    assert_eq!(
        fixture.project()["workspace"],
        "/changed-after-unknown-effect"
    );
    let connection = fixture.connection();
    let (status,count):(String,u64)=connection.query_row("SELECT (SELECT status FROM repository_operations),(SELECT count(*) FROM repository_worktrees)",[],|r|Ok((r.get(0)?,r.get(1)?))).unwrap();
    assert_eq!((status.as_str(), count), ("needs_reconciliation", 0));
}

#[test]
fn a_late_connected_receipt_cannot_override_a_newer_operation_decision() {
    let root = tempfile::tempdir().unwrap();
    let fixture = Fixture::new(root.path());
    let observation = fixture.observe(fixture.connected());
    let connection = fixture.connection();
    connection.execute("UPDATE repository_operations SET status='needs_reconciliation',revision=revision+1,reason='restart_after_dispatch'",[]).unwrap();
    assert_eq!(
        accept_connect_observation(fixture.store.as_ref(), &observation)
            .unwrap_err()
            .code,
        "revision_conflict"
    );
    assert_eq!(fixture.project()["workspace"], "/unattached");
    let (status,revision,worktrees,heads):(String,u64,u64,u64)=connection.query_row("SELECT status,revision,(SELECT count(*) FROM repository_worktrees),(SELECT count(*) FROM heads) FROM repository_operations",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).unwrap();
    assert_eq!(
        (status.as_str(), revision, worktrees, heads),
        ("needs_reconciliation", 3, 0, 0)
    );
    assert!(
        fixture
            .store
            .read_resources(&[ResourceKey::new("governance", &observation)])
            .unwrap()[0]
            .value
            .is_some()
    );
}
