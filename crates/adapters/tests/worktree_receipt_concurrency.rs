//! Deterministic external-port race: a business status changes during Git I/O.
use devdesk_adapters::{FileArtifacts, SqliteStore};
use devdesk_core::{concurrency::*, *};
use devdesk_domain::*;
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
#[path = "../../../tests/support/runtime.rs"]
mod runtime;
struct RacingGit {
    store: Arc<SqliteStore>,
    kind: &'static str,
    complete: bool,
    calls: AtomicUsize,
}
impl ports::Git for RacingGit {
    fn repository_operation(&self, op: &RepositoryOperation) -> Result<RepositoryReceipt> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let RepositoryAction::CreateWorktree {
            owner,
            worktree_kind,
            base,
            ..
        } = &op.action
        else {
            panic!("unexpected operation")
        };
        let key = ResourceKey::new(self.kind, owner);
        let row = self
            .store
            .read_resources(std::slice::from_ref(&key))?
            .remove(0);
        let mut value = row.value.unwrap();
        value["status"] = json!(if self.complete {
            "completed"
        } else {
            "running"
        });
        value["revision"] = json!(row.revision + 1);
        self.store.commit_changes(&PreparedChangeSet {
            command_id: "during-git".into(),
            fingerprint: configuration_hash(&value),
            read_set: ReadSet::new(vec![ResourceVersion {
                resource: key.clone(),
                revision: row.revision,
            }])?,
            changes: vec![ResourceChange {
                resource: key,
                value,
            }],
            result: Value::Null,
            time: 2,
        })?;
        Ok(RepositoryReceipt::Worktree {
            worktree: RepositoryWorktree {
                id: owner.clone(),
                owner_id: owner.clone(),
                project_id: op.project_id.clone(),
                kind: *worktree_kind,
                branch: format!("fixture/{owner}"),
                path: format!(
                    "{}/execution/worktrees/{}/{owner}",
                    self.store.root(),
                    if self.kind == "task" {
                        "tasks"
                    } else {
                        "features"
                    }
                ),
                base: base.clone(),
                operation_id: op.id.clone(),
            },
        })
    }
    fn candidate(&self, _: &GitContext, _: &str, _: &str, _: &str) -> Result<String> {
        unreachable!()
    }
    fn advance(&self, _: &GitContext, _: &str, _: &str, _: &str, _: &str) -> Result<String> {
        unreachable!()
    }
}
fn race(kind: &'static str, complete: bool) {
    let dir = tempfile::tempdir().unwrap();
    let project = "58ef74cd-ec28-46cf-ae7d-277d65d272c7";
    let store = Arc::new(SqliteStore::open_project(dir.path(), project).unwrap());
    let git = Arc::new(RacingGit {
        store: store.clone(),
        kind,
        complete,
        calls: AtomicUsize::new(0),
    });
    let mut adapters = runtime::adapters();
    adapters.git = git.clone();
    let e = Engine::new(
        store.clone(),
        Arc::new(FileArtifacts::new(store.root().into())),
        adapters,
    );
    e.bootstrap().unwrap();
    e.execute(
        "project.create",
        Command {
            command_id: "project".into(),
            target: None,
            expected_revision: None,
            payload: json!({"title":"Worktree race"}),
        },
    )
    .unwrap();
    store
        .seed_fixture_state(None, &mut |s| {
            s.features.push(Feature {
                id: "feature".into(),
                project_id: project.into(),
                title: "Feature".into(),
                status: Status::Draft,
                revision: 1,
                input_version: 1,
                predecessor: None,
                source_materials: vec![],
            });
            if kind == "task" {
                s.tasks.push(Task {
                    id: "task".into(),
                    feature_id: "feature".into(),
                    title: "Task".into(),
                    status: Status::Draft,
                    revision: 1,
                    inherited_snapshot: None,
                    dependencies: vec![],
                    worktree: String::new(),
                    scope: Default::default(),
                });
            }
            s.repository_operations.push(RepositoryOperation {
                id: "create-worktree".into(),
                project_id: project.into(),
                action: RepositoryAction::CreateWorktree {
                    owner: kind.into(),
                    worktree_kind: if kind == "task" {
                        WorktreeKind::Task
                    } else {
                        WorktreeKind::Feature
                    },
                    base: "a".repeat(40),
                    branch: None,
                },
                status: RepositoryOperationStatus::Prepared,
                receipt: None,
                reason: String::new(),
                revision: 1,
            });
            Ok(Value::Null)
        })
        .unwrap();
    for _ in 0..3 {
        e.tick_repositories().unwrap();
    }
    let s = store.snapshot().unwrap();
    let op = s
        .repository_operations
        .iter()
        .find(|o| o.id == "create-worktree")
        .unwrap();
    assert_eq!(
        git.calls.load(Ordering::SeqCst),
        1,
        "never repeat a granted physical operation"
    );
    if complete {
        assert_eq!(op.status, RepositoryOperationStatus::NeedsReconciliation);
        assert!(s.repository_worktrees.is_empty());
    } else {
        assert_eq!(
            op.status,
            RepositoryOperationStatus::Completed,
            "{}",
            op.reason
        );
        assert_eq!(s.repository_worktrees.len(), 1);
        if kind == "task" {
            assert_eq!(s.tasks[0].status, Status::Running);
            assert!(!s.tasks[0].worktree.is_empty());
        }
    }
}
#[test]
fn feature_progress_during_git_does_not_strand_the_worktree_receipt() {
    race("feature", false);
}
#[test]
fn task_progress_during_git_preserves_status_and_admits_the_original_receipt() {
    race("task", false);
}
#[test]
fn completed_owner_revokes_receipt_admission_without_repeating_git() {
    race("feature", true);
    race("task", true);
}
