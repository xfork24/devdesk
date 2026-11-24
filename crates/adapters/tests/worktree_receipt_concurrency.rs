//! Deterministic control-plane changes during repository effects.
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

struct ConnectGit {
    store: Arc<SqliteStore>,
    during: Value,
    calls: AtomicUsize,
}

impl ports::Git for ConnectGit {
    fn repository_operation(&self, op: &RepositoryOperation) -> Result<RepositoryReceipt> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        edit_project(self.store.as_ref(), self.during.clone());
        Ok(connect_receipt(op, &self.store.root()))
    }
    fn inspect_repository_operation(
        &self,
        _: &RepositoryOperation,
    ) -> Result<Option<RepositoryReceipt>> {
        panic!("the saved successful observation must be used");
    }
    fn candidate(&self, _: &GitContext, _: &str, _: &str, _: &str) -> Result<String> {
        unreachable!()
    }
    fn advance(&self, _: &GitContext, _: &str, _: &str, _: &str, _: &str) -> Result<String> {
        unreachable!()
    }
}

fn edit_project(store: &SqliteStore, payload: Value) {
    let project = store.project_metadata().unwrap();
    let context = InvocationContext::human(Some(project.id.clone()));
    let command = Command {
        command_id: id(),
        target: Some(project.id),
        expected_revision: Some(project.revision),
        payload,
    };
    let patch = project_updates::ProjectUpdateContext::read(store, &command)
        .unwrap()
        .prepare(&command, &context)
        .unwrap();
    store.commit_changes(&patch).unwrap();
}

fn connect_receipt(op: &RepositoryOperation, root: &str) -> RepositoryReceipt {
    let RepositoryAction::Connect {
        repository,
        mainline,
    } = &op.action
    else {
        panic!("expected repository connect")
    };
    let project = &op.project_id;
    let head = "b".repeat(40);
    RepositoryReceipt::Connected {
        policy: GitHubRepositoryPolicy {
            repository: repository.clone(),
            repository_id: "fixture".into(),
            branch: mainline.clone(),
            head: Some(head.clone()),
            viewer_permission: "ADMIN".into(),
            compatible: true,
            blocked_reasons: vec![],
            active_branch_rulesets: vec![],
            classic_protection_count: 0,
            observed_at: 1,
        },
        workspace: Workspace {
            repository: repository.clone(),
            mainline: mainline.clone(),
            path: format!("{root}/repository"),
        },
        head: head.clone(),
        worktree: RepositoryWorktree {
            id: project.clone(),
            project_id: project.clone(),
            owner_id: project.clone(),
            kind: WorktreeKind::Mainline,
            branch: mainline.clone(),
            path: format!("{root}/execution/worktrees/mainlines/{project}"),
            base: head,
            operation_id: op.id.clone(),
        },
    }
}

fn prepared_connection(root: &std::path::Path, during: Value) -> (Engine, Arc<ConnectGit>) {
    let project = id();
    let store = Arc::new(SqliteStore::open_project(root, &project).unwrap());
    let git = Arc::new(ConnectGit {
        store: store.clone(),
        during,
        calls: AtomicUsize::new(0),
    });
    let mut adapters = runtime::adapters();
    adapters.git = git.clone();
    let engine = Engine::new(
        store.clone(),
        Arc::new(FileArtifacts::new(store.root().into())),
        adapters,
    );
    engine.bootstrap().unwrap();
    engine.execute("project.create", Command {
        command_id: "connect-project".into(),
        target: None,
        expected_revision: None,
        payload: json!({"title":"Original project","repository":"fixture/repo","mainline":"main"}),
    }).unwrap();
    engine
        .execute(
            "repository.connect",
            Command {
                command_id: "connect-operation".into(),
                target: Some(project),
                expected_revision: Some(1),
                payload: json!({"repository":"fixture/repo","mainline":"main"}),
            },
        )
        .unwrap();
    (engine, git)
}

#[test]
fn repository_connect_keeps_metadata_edits_before_and_during_git() {
    let root = tempfile::tempdir().unwrap();
    let (engine, git) = prepared_connection(root.path(), json!({"concurrency":7}));
    edit_project(
        git.store.as_ref(),
        json!({"title":"Name changed before dispatch"}),
    );
    engine.tick_repositories().unwrap();
    engine.tick_repositories().unwrap();
    let state = engine.store.snapshot().unwrap();
    let project = &state.projects[0];
    assert_eq!(project.title, "Name changed before dispatch");
    assert_eq!(project.concurrency, 7);
    assert_eq!(project.revision, 4);
    assert_eq!(state.repository_operations.len(), 1);
    assert_eq!(
        state.repository_operations[0].status,
        RepositoryOperationStatus::Completed
    );
    assert_eq!(state.heads[0].revision, "b".repeat(40));
    assert_eq!(state.repository_worktrees.len(), 1);
    assert_eq!(git.calls.load(Ordering::SeqCst), 1);
}

#[test]
fn repository_connect_cannot_overwrite_a_target_changed_during_git() {
    let root = tempfile::tempdir().unwrap();
    let (engine, git) = prepared_connection(
        root.path(),
        json!({
            "workspace_definition":{"repository":"fixture/another","mainline":"main","path":""}
        }),
    );
    engine.tick_repositories().unwrap();
    let state = engine.store.snapshot().unwrap();
    assert_eq!(
        state.projects[0]
            .workspace_definition
            .as_ref()
            .unwrap()
            .repository,
        "fixture/another"
    );
    assert_eq!(
        state.repository_operations[0].status,
        RepositoryOperationStatus::NeedsReconciliation
    );
    assert!(
        state.repository_operations[0]
            .reason
            .contains("repository_target_changed")
    );
    assert!(state.heads.is_empty());
    assert!(state.repository_worktrees.is_empty());
    assert!(
        state
            .governance
            .iter()
            .any(|record| record.id.starts_with("repository-observation:"))
    );
    assert_eq!(git.calls.load(Ordering::SeqCst), 1);
}

#[test]
fn legacy_repository_connect_conflict_recovers_from_its_saved_observation() {
    let root = tempfile::tempdir().unwrap();
    let (engine, git) = prepared_connection(root.path(), Value::Null);
    edit_project(
        git.store.as_ref(),
        json!({"title":"Saved after the original authorization"}),
    );
    engine.store.seed_fixture_state(None, &mut |state| {
        let op = &mut state.repository_operations[0];
        let mut dispatched = op.clone();
        dispatched.status = RepositoryOperationStatus::Dispatched;
        dispatched.revision = 2;
        op.status = RepositoryOperationStatus::NeedsReconciliation;
        op.reason = "原物理观察已保留，原业务接纳版本冲突".into();
        op.revision = 3;
        let authority = state.governance.iter()
            .find(|record| record.r#type == "repository_authorization").unwrap().clone();
        let basis: ReadSet = serde_json::from_value(authority.data["basis"].clone()).unwrap();
        let basis = basis.merge([
            ResourceVersion { resource: ResourceKey::new("governance", &authority.id), revision: 1 },
            ResourceVersion { resource: ResourceKey::new("head", format!("main:{}", op.project_id)), revision: 0 },
            ResourceVersion { resource: ResourceKey::new("repository_worktree", &op.project_id), revision: 0 },
        ])?;
        let effect_id = format!("repository-effect:{}", op.id);
        let original_version = ResourceVersion {
            resource: ResourceKey::new("repository_operation", &op.id), revision: 2,
        };
        let fact = |id: String, kind: &str, data: Value| ConfigurationRecord {
            id, r#type: kind.into(), content_hash: content_digest(&authority.refs, &data),
            data, ..authority.clone()
        };
        state.governance.push(fact(effect_id.clone(), "repository_effect_intent", json!({
            "operation":dispatched,"operation_version":original_version,"basis":basis,"issuer":"kernel_repository"
        })));
        state.governance.push(fact(format!("repository-observation:{}", op.id), "repository_effect_observation", json!({
            "effect_id":effect_id,"operation_version":original_version,
            "result":{"Ok":connect_receipt(&dispatched, &git.store.root())},"issuer":"external_git"
        })));
        Ok(Value::Null)
    }).unwrap();
    assert!(engine.store.has_pending_work().unwrap());
    engine.tick_repositories().unwrap();
    let state = engine.store.snapshot().unwrap();
    assert_eq!(
        state.projects[0].title,
        "Saved after the original authorization"
    );
    assert_eq!(
        state.repository_operations[0].status,
        RepositoryOperationStatus::Completed
    );
    assert_eq!(state.repository_worktrees.len(), 1);
    assert_eq!(git.calls.load(Ordering::SeqCst), 0);
}
