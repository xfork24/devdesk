use devdesk_adapters::CliGit;
use devdesk_core::id;
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
    // A delivery patch must retain non-text changes and the complete object IDs.
    let task_path = Path::new(&context.source.worktree);
    fs::rename(
        task_path.join("addition.txt"),
        task_path.join("renamed.txt"),
    )
    .unwrap();
    fs::remove_file(task_path.join("base.txt")).unwrap();
    fs::write(task_path.join("binary.bin"), [0, 1, 2, 0, 255, 128]).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::{PermissionsExt, symlink};
        fs::set_permissions(
            task_path.join("renamed.txt"),
            fs::Permissions::from_mode(0o755),
        )
        .unwrap();
        symlink("renamed.txt", task_path.join("link")).unwrap();
    }
    let changed = adapter.snapshot_worktree(&context, &source, &id()).unwrap();
    let patch = adapter.diff(&context, &source, &changed).unwrap();
    assert!(patch.contains("GIT binary patch"));
    assert!(patch.contains("rename from addition.txt"));
    assert!(patch.contains("deleted file mode"));
    #[cfg(unix)]
    {
        assert!(patch.contains("100755"));
        assert!(patch.contains("120000"));
    }
    assert_eq!(
        patch.trim(),
        git(
            &repo,
            &[
                "diff",
                "--binary",
                "--full-index",
                "--find-renames",
                &source,
                &changed,
                "--"
            ]
        )
    );
    assert_eq!(adapter.diff(&context, &changed, &changed).unwrap(), "");
    assert!(adapter.diff(&context, "HEAD", &changed).is_err());
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

#[test]
fn delivery_sync_preserves_dirty_worktrees_and_recovers_a_lost_checkout() {
    let root = tempfile::tempdir().unwrap();
    let project = id();
    let repo = root
        .path()
        .join("projects")
        .join(&project)
        .join("repository");
    fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-b", "main"]);
    fs::write(repo.join("base.txt"), "base\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "baseline"]);
    let base = git(&repo, &["rev-parse", "HEAD"]);
    let remote = root.path().join("remote.git");
    git(root.path(), &["init", "--bare", remote.to_str().unwrap()]);
    git(
        &repo,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    let adapter = CliGit::new(root.path()).unwrap();
    let feature = adapter
        .prepare_worktree(&project, &id(), "features", &base, &id())
        .unwrap();
    let task = adapter
        .prepare_worktree(&project, &id(), "tasks", &base, &id())
        .unwrap();
    let context = GitContext {
        project: project.clone(),
        workspace: Workspace {
            repository: "fixture/local".into(),
            path: repo.to_string_lossy().into(),
            mainline: "main".into(),
        },
        source: CodeLocation {
            owner: task.owner.clone(),
            branch: task.branch,
            worktree: task.path.clone(),
        },
        target: CodeLocation {
            owner: feature.owner.clone(),
            branch: feature.branch.clone(),
            worktree: feature.path.clone(),
        },
        origin: Origin::External,
    };
    fs::write(
        Path::new(&task.path).join("addition.txt"),
        "verified source\n",
    )
    .unwrap();
    let source = adapter.snapshot_worktree(&context, &base, &id()).unwrap();
    let candidate = adapter.candidate(&context, &source, &base, &id()).unwrap();
    git(
        &repo,
        &[
            "push",
            "origin",
            &format!("{candidate}:refs/heads/accepted"),
        ],
    );
    let target = Path::new(&feature.path);
    fs::write(target.join("local.txt"), "keep this edit\n").unwrap();
    assert!(
        !adapter
            .delivery_sync_ready(&context, &base, &candidate)
            .unwrap()
    );
    let original = id();
    assert!(
        adapter
            .sync_delivery(&context, &base, &candidate, &original, false)
            .is_err()
    );
    assert_eq!(
        fs::read_to_string(target.join("local.txt")).unwrap(),
        "keep this edit\n"
    );
    assert_eq!(git(target, &["rev-parse", "HEAD"]), base);
    fs::remove_file(target.join("local.txt")).unwrap();
    // Simulate the physical crash boundary after ref CAS, before checkout.
    git(
        &repo,
        &[
            "update-ref",
            &format!("refs/heads/{}", feature.branch),
            &candidate,
            &base,
        ],
    );
    let observed = adapter
        .sync_delivery(&context, &base, &candidate, &original, true)
        .unwrap()
        .unwrap();
    assert_eq!(observed["checkout_required"], true);
    assert!(!target.join("addition.txt").exists());
    assert!(
        adapter
            .delivery_sync_ready(&context, &base, &candidate)
            .unwrap()
    );
    let recovered = adapter
        .sync_delivery(&context, &base, &candidate, &id(), false)
        .unwrap()
        .unwrap();
    assert_eq!(recovered["synchronized"], true);
    assert_eq!(
        fs::read_to_string(target.join("addition.txt")).unwrap(),
        "verified source\n"
    );
    assert_eq!(
        adapter
            .sync_delivery(&context, &base, &candidate, &original, true)
            .unwrap()
            .unwrap()["synchronized"],
        true
    );
    assert_eq!(git(Path::new(&task.path), &["rev-parse", "HEAD"]), source);
    assert!(git(target, &["status", "--porcelain"]).is_empty());
    fs::write(target.join("unknown.txt"), "unrelated successor\n").unwrap();
    git(target, &["add", "."]);
    git(target, &["commit", "-m", "unknown successor"]);
    assert!(
        adapter
            .delivery_sync_ready(&context, &base, &candidate)
            .is_err()
    );
    assert!(target.join("unknown.txt").exists());
}

#[test]
fn pr_test_merge_is_fetched_and_pinned_locally_without_remote_candidate_branches() {
    let root = tempfile::tempdir().unwrap();
    let project = id();
    let feature = id();
    let repo = root
        .path()
        .join("projects")
        .join(&project)
        .join("repository");
    let remote = root.path().join("remote.git");
    fs::create_dir_all(&repo).unwrap();
    fs::create_dir_all(&remote).unwrap();
    git(&repo, &["init", "-b", "main"]);
    git(&remote, &["init", "--bare"]);
    fs::write(repo.join("base.txt"), "base\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "base"]);
    let base = git(&repo, &["rev-parse", "HEAD"]);
    git(
        &repo,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    let adapter = CliGit::new(root.path()).unwrap();
    let work = adapter
        .prepare_worktree(&project, &feature, "features", &base, &id())
        .unwrap();
    let path = Path::new(&work.path);
    fs::write(path.join("feature.txt"), "feature\n").unwrap();
    git(path, &["add", "."]);
    git(path, &["commit", "-m", "feature"]);
    let source = git(path, &["rev-parse", "HEAD"]);
    let tree = git(path, &["rev-parse", "HEAD^{tree}"]);
    git(&repo, &["push", "origin", "main", &work.branch]);
    // GitHub owns refs/pull/.../merge. Simulate that server-side object only.
    let merge = git(
        &remote,
        &[
            "commit-tree",
            &tree,
            "-p",
            &base,
            "-p",
            &source,
            "-m",
            "GitHub test merge",
        ],
    );
    git(&remote, &["update-ref", "refs/pull/7/merge", &merge]);
    struct Daemon(std::process::Child);
    impl Drop for Daemon {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let _daemon = Daemon(
        Command::new("git")
            .args([
                "daemon",
                "--reuseaddr",
                "--export-all",
                "--listen=127.0.0.1",
                &format!("--port={port}"),
                &format!("--base-path={}", root.path().display()),
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap(),
    );
    for _ in 0..100 {
        if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    git(
        &repo,
        &[
            "remote",
            "set-url",
            "origin",
            &format!("git://127.0.0.1:{port}/remote.git"),
        ],
    );
    let context = GitContext {
        project: project.clone(),
        workspace: Workspace {
            repository: "fixture/local".into(),
            path: fs::canonicalize(&repo).unwrap().to_string_lossy().into(),
            mainline: "main".into(),
        },
        source: CodeLocation {
            owner: feature,
            branch: work.branch.clone(),
            worktree: work.path.clone(),
        },
        target: CodeLocation {
            owner: project,
            branch: "main".into(),
            worktree: fs::canonicalize(&repo).unwrap().to_string_lossy().into(),
        },
        origin: Origin::External,
    };
    for _ in 0..2 {
        adapter.fetch_delivery_commit(&context, &merge).unwrap();
    }
    assert_eq!(
        git(
            &repo,
            &["rev-parse", &format!("refs/devdesk/pr-candidates/{merge}")]
        ),
        merge
    );
    assert_eq!(
        adapter.commit_parents(&context, &merge).unwrap(),
        vec![base.clone(), source.clone()]
    );
    assert_eq!(adapter.commit_tree(&context, &merge).unwrap(), tree);
    assert_eq!(git(&repo, &["rev-parse", "main"]), base);
    assert_eq!(git(&repo, &["rev-parse", &work.branch]), source);
    let refs = git(
        &remote,
        &["for-each-ref", "--format=%(refname)", "refs/heads/"],
    );
    assert!(!refs.contains("devdesk/candidates/"));
    let operation = RepositoryOperation {
        id: id(),
        project_id: context.project.clone(),
        action: RepositoryAction::PublishRef {
            merge: id(),
            context: context.clone(),
            branch: format!("devdesk/candidates/{}", id()),
            revision: merge,
            expected: None,
            stage: PublishStage::DeliveryCandidate,
        },
        status: RepositoryOperationStatus::Prepared,
        receipt: None,
        reason: String::new(),
        revision: 1,
    };
    assert_eq!(
        adapter
            .repository_operation(&operation)
            .unwrap_err()
            .execution_failure
            .unwrap()
            .effect,
        ExecutionFailureEffect::NotApplied
    );
    assert_eq!(
        git(
            &remote,
            &["for-each-ref", "--format=%(refname)", "refs/heads/"]
        ),
        refs
    );
}
