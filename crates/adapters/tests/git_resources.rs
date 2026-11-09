use devdesk_adapters::CliGit;
use devdesk_core::ports::Git;
use devdesk_domain::*;
use std::{fs, path::Path, process::Command};
fn git(path: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args([
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@localhost",
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
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().into()
}
#[test]
fn actual_worktrees_single_parent_candidate_cas_and_recovery() {
    let root = tempfile::tempdir().unwrap();
    let project = id();
    let project_path = root.path().join("projects").join(&project);
    let repo = project_path.join("repository");
    fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-b", "main"]);
    fs::write(repo.join("base.txt"), "base\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "Fixture baseline"]);
    let base = git(&repo, &["rev-parse", "HEAD"]);
    let adapter = CliGit::new(root.path()).unwrap();
    let feature = id();
    let task = id();
    let feature_op = id();
    let f = adapter
        .prepare_worktree(&project, &feature, "features", &base, &feature_op)
        .unwrap();
    assert_eq!(
        adapter
            .prepare_worktree(&project, &feature, "features", &base, &feature_op)
            .unwrap()
            .path,
        f.path
    );
    let t = adapter
        .prepare_worktree(&project, &task, "tasks", &base, &id())
        .unwrap();
    let context = GitContext {
        project: project.clone(),
        workspace: Workspace {
            repository: "fixture/local".into(),
            path: fs::canonicalize(&repo).unwrap().to_string_lossy().into(),
            mainline: "main".into(),
        },
        source: CodeLocation {
            owner: task,
            branch: t.branch.clone(),
            worktree: t.path.clone(),
        },
        target: CodeLocation {
            owner: feature,
            branch: f.branch.clone(),
            worktree: f.path.clone(),
        },
        origin: Origin::External,
    };
    fs::write(Path::new(&t.path).join("addition.txt"), "2+3=5\n").unwrap();
    let snap_op = id();
    let source = adapter
        .snapshot_worktree(&context, &base, &snap_op)
        .unwrap();
    assert_eq!(
        adapter
            .snapshot_worktree(&context, &base, &snap_op)
            .unwrap(),
        source
    );
    assert_ne!(source, base);
    let candidate_op = id();
    let candidate = adapter
        .candidate(&context, &source, &base, &candidate_op)
        .unwrap();
    assert_eq!(
        adapter
            .candidate(&context, &source, &base, &candidate_op)
            .unwrap(),
        candidate
    );
    assert_eq!(
        git(&repo, &["rev-list", "--parents", "-n", "1", &candidate]),
        format!("{candidate} {base}")
    );
    assert_eq!(
        adapter.commit_parents(&context, &candidate).unwrap(),
        vec![base.clone()]
    );
    assert!(adapter.commit_parents(&context, "HEAD").is_err());
    assert!(
        adapter
            .diff(&context, &base, &candidate)
            .unwrap()
            .contains("addition.txt")
    );
    // A domain Merge ID names both preparation and advancement; purpose namespaces must differ.
    let op = candidate_op.clone();
    assert_eq!(
        adapter
            .advance(&context, &base, &base, &candidate, &op)
            .unwrap(),
        candidate
    );
    assert!(Path::new(&f.path).join("addition.txt").is_file());
    assert!(git(Path::new(&f.path), &["status", "--porcelain"]).is_empty());
    assert_eq!(
        adapter
            .advance(&context, &base, &base, &candidate, &op)
            .unwrap(),
        candidate
    );
    assert!(
        matches!(adapter.reconcile_advance(&context,&op).unwrap(),AdvanceObservation::Applied{revision} if revision==candidate)
    );
    assert!(
        adapter
            .advance(&context, &base, &base, &candidate, &id())
            .is_err()
    );
    assert_eq!(
        git(&repo, &["rev-parse", &format!("refs/heads/{}", f.branch)]),
        candidate
    );
    assert_eq!(
        git(&repo, &["rev-parse", &format!("refs/heads/{}", t.branch)]),
        source
    );
    assert!(
        adapter
            .candidate(&context, &source, &candidate, &candidate_op)
            .is_err()
    );
}
#[test]
fn dirty_targets_and_foreign_contexts_are_rejected_without_modifying_refs() {
    let root = tempfile::tempdir().unwrap();
    let project = id();
    let repo = root
        .path()
        .join("projects")
        .join(&project)
        .join("repository");
    fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-b", "main"]);
    fs::write(repo.join("base"), "base").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "base"]);
    let base = git(&repo, &["rev-parse", "HEAD"]);
    let a = CliGit::new(root.path()).unwrap();
    let f = a
        .prepare_worktree(&project, &id(), "features", &base, &id())
        .unwrap();
    let t = a
        .prepare_worktree(&project, &id(), "tasks", &base, &id())
        .unwrap();
    let c = GitContext {
        project,
        workspace: Workspace {
            repository: "local".into(),
            path: fs::canonicalize(&repo).unwrap().to_string_lossy().into(),
            mainline: "main".into(),
        },
        source: CodeLocation {
            owner: t.owner,
            branch: t.branch,
            worktree: t.path,
        },
        target: CodeLocation {
            owner: f.owner,
            branch: f.branch,
            worktree: f.path,
        },
        origin: Origin::External,
    };
    let candidate = a.candidate(&c, &base, &base, &id()).unwrap();
    fs::write(Path::new(&c.target.worktree).join("untracked"), "keep me").unwrap();
    assert!(a.advance(&c, &base, &base, &candidate, &id()).is_err());
    assert_eq!(
        git(
            &repo,
            &["rev-parse", &format!("refs/heads/{}", c.target.branch)]
        ),
        base
    );
    let mut wrong = c.clone();
    wrong.workspace.path = "/tmp/foreign".into();
    assert!(a.candidate(&wrong, &base, &base, &id()).is_err());
}

#[test]
fn interrupted_clone_rejects_foreign_origin_without_changing_repository() {
    let root = tempfile::tempdir().unwrap();
    let project = id();
    let repo = root
        .path()
        .join("projects")
        .join(&project)
        .join("repository");
    fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "--bare"]);
    git(
        &repo,
        &[
            "remote",
            "add",
            "origin",
            "https://github.com/foreign/repository.git",
        ],
    );
    fs::write(repo.join("preserved.txt"), "existing repository").unwrap();
    let adapter = CliGit::new(root.path()).unwrap();
    let error = adapter
        .clone_repository(&project, "owner/repository", "main", &id())
        .unwrap_err();
    assert!(error.message.contains("origin"));
    assert_eq!(
        git(&repo, &["remote", "get-url", "origin"]),
        "https://github.com/foreign/repository.git"
    );
    assert_eq!(
        fs::read_to_string(repo.join("preserved.txt")).unwrap(),
        "existing repository"
    );
    assert!(git(&repo, &["for-each-ref"]).is_empty());
}
