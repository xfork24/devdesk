//! Test-only seed of a private local repository from actual Git receipts.
use super::*;
use crate::workspace::Workspace as HostWorkspace;
use devdesk_adapters::CliGit;
use devdesk_domain::*;
use std::path::Path;

fn git(path: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .args([
            "-c",
            "user.name=Workflow probe",
            "-c",
            "user.email=probe@localhost",
            "-c",
            "commit.gpgSign=false",
            "-c",
            "core.hooksPath=/dev/null",
            "-C",
        ])
        .arg(path)
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim().into()
}

pub(super) fn seed(workspace: &Arc<HostWorkspace>, root: &Path, project: &str) {
    let e = workspace.project_engine(Some(project)).unwrap();
    let repo = root.join("projects").join(project).join("repository");
    std::fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-b", "main"]);
    std::fs::write(repo.join("probe.test.mjs"), "import {test} from 'node:test'; import assert from 'node:assert/strict'; test('actual frozen workflow input',()=>assert.equal(2+3,5));\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "isolated workflow probe input"]);
    let base = git(&repo, &["rev-parse", "HEAD"]);
    let adapter = CliGit::new(root).unwrap();
    let work = adapter
        .prepare_mainline(project, "integration-main", &base, &id())
        .unwrap();
    // Seed the dedicated local scene from actual Git receipts. There is no local-repository
    // onboarding RPC; the production GitHub onboarding flow is outside this credential-free test.
    e.store
        .seed_fixture_state(None, &mut |s| {
            let p = s.projects.iter_mut().find(|p| p.id == project).unwrap();
            p.workspace_definition = Some(devdesk_domain::Workspace {
                repository: "probe/local".into(),
                path: repo.to_string_lossy().into(),
                mainline: "integration-main".into(),
            });
            s.heads.push(GitHead {
                id: format!("main:{project}"),
                project_id: project.to_owned(),
                revision: base.clone(),
            });
            let wt = RepositoryWorktree {
                id: work.owner.clone(),
                owner_id: work.owner.clone(),
                project_id: project.to_owned(),
                kind: WorktreeKind::Mainline,
                branch: work.branch.clone(),
                path: work.path.clone(),
                base: base.clone(),
                operation_id: work.operation.clone(),
            };
            s.repository_operations.push(RepositoryOperation {
                id: work.operation.clone(),
                project_id: project.to_owned(),
                action: RepositoryAction::CreateWorktree {
                    owner: work.owner.clone(),
                    worktree_kind: WorktreeKind::Mainline,
                    base: base.clone(),
                    branch: None,
                },
                status: RepositoryOperationStatus::Completed,
                receipt: Some(RepositoryReceipt::Worktree {
                    worktree: wt.clone(),
                }),
                reason: String::new(),
                revision: 1,
            });
            s.repository_worktrees.push(wt);
            Ok(Value::Null)
        })
        .unwrap();
    std::fs::write(
        Path::new(&work.path).join("note.txt"),
        "actual snapshot input\n",
    )
    .unwrap();
}

pub(super) fn feature(engine: &Engine, owner: &str) {
    // feature.create already persisted the sole worktree intent. Drive that
    // exact intent through the real port; never invent a second operation.
    for _ in 0..3 {
        engine.tick_repositories().unwrap();
    }
    let state = engine.store.snapshot().unwrap();
    let worktree = state
        .repository_worktrees
        .iter()
        .find(|w| w.owner_id == owner)
        .expect("actual admitted Feature worktree");
    std::fs::write(
        Path::new(&worktree.path).join("note.txt"),
        "independent real snapshot input\n",
    )
    .unwrap();
}
