//! Explicit local integration scene: real Host scheduler, Git, Docker, SQLite and artifacts.
//! No model, credentials, network service or business Feature is used.
use super::*;
use std::time::{Duration, Instant};

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

#[test]
#[ignore = "explicit actual Git/Docker workflow; DEVDESK_WORKFLOW_LIVE_OUTPUT must be a new directory"]
fn actual_git_docker_workflow_handoffs() {
    let output = PathBuf::from(
        std::env::var_os("DEVDESK_WORKFLOW_LIVE_OUTPUT").expect("evidence directory"),
    );
    std::fs::create_dir(&output).expect("new evidence directory");
    let root = output.join("data");
    let w = Workspace::open(&root).unwrap();
    let root = std::fs::canonicalize(root).unwrap();
    let project = tests::create(&w, "Dedicated workflow transport probe");
    let e = w.project_engine(Some(&project)).unwrap();
    crate::test_runtime::retired_configuration::install(&e);
    let command = |method: &str, target: Option<&str>, payload: Value| {
        let state = e.store.snapshot().unwrap();
        let global = w.global.snapshot().unwrap();
        let revision = target.and_then(|id| {
            state
                .configuration(id)
                .ok()
                .map(|r| r.revision)
                .or_else(|| {
                    state
                        .projects
                        .iter()
                        .find(|p| p.id == id)
                        .map(|p| p.revision)
                })
                .or_else(|| global.configuration(id).ok().map(|r| r.revision))
        });
        w.dispatch(method,json!({"project_id":project,"command_id":id(),"target":target,"expected_revision":revision,"payload":payload})).unwrap()
    };
    let repo = root.join("projects").join(&project).join("repository");
    std::fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-b", "main"]);
    std::fs::write(repo.join("probe.test.mjs"), "import {test} from 'node:test'; import assert from 'node:assert/strict'; test('actual frozen workflow input',()=>assert.equal(2+3,5));\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "isolated workflow probe input"]);
    let base = git(&repo, &["rev-parse", "HEAD"]);
    let adapter = CliGit::new(&root).unwrap();
    let work = adapter
        .prepare_mainline(&project, "integration-main", &base, &id())
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
                project_id: project.clone(),
                revision: base.clone(),
            });
            let wt = RepositoryWorktree {
                id: work.owner.clone(),
                owner_id: work.owner.clone(),
                project_id: project.clone(),
                kind: WorktreeKind::Mainline,
                branch: work.branch.clone(),
                path: work.path.clone(),
                base: base.clone(),
                operation_id: work.operation.clone(),
            };
            s.repository_operations.push(RepositoryOperation {
                id: work.operation.clone(),
                project_id: project.clone(),
                action: RepositoryAction::CreateWorktree {
                    owner: work.owner.clone(),
                    worktree_kind: WorktreeKind::Mainline,
                    base: base.clone(),
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
    let mut steps = vec![
        json!({"key":"current","name":"Read actual Git revision","stage":"probe","executor":{"type":"tool","tool_ref":"builtin-tool-workspace-current:2","arguments":{"owner_id":{"type":"constant","value":"$project"}}},"outputs":{"workspace":{"delivery_ref":"builtin-delivery-workspace-snapshot:3"}}}),
        json!({"key":"snapshot","name":"Freeze actual Git files","stage":"probe","dependencies":["current"],"executor":{"type":"tool","tool_ref":"builtin-tool-workspace-snapshot:2","arguments":{"workspace":{"type":"source","source":{"step":"current","port":"workspace"}},"allowed_paths":{"type":"constant","value":["note.txt"]},"include_diff":{"type":"constant","value":false}}},"outputs":{"workspace":{"delivery_ref":"builtin-delivery-workspace-snapshot:3"}}}),
    ];
    for n in 0..6 {
        steps.push(json!({"key":format!("docker_{n}"),"name":format!("Actual Docker test {n}"),"stage":"probe","dependencies":[if n==0 {"snapshot".into()}else{format!("docker_{}",n-1)}],"executor":{"type":"tool","tool_ref":"simplified-tool-tests-run:3","arguments":{"workspace":{"type":"source","source":{"step":"snapshot","port":"workspace"}},"recipe":{"type":"constant","value":{"runner":"node_test","files":["probe.test.mjs"],"timeout_seconds":30}}}},"outputs":{"report":{"delivery_ref":"builtin-delivery-test-report:2"}},"completion":{"op":"equals","source":{"step":format!("docker_{n}"),"port":"report","pointer":"/status"},"value":"succeeded"}}));
    }
    let draft = command(
        "workflow.create",
        None,
        json!({"name":"Actual Git Docker handoffs","scope":format!("project:{project}"),"document":{"schema_version":6,"scheduling":{"max_actions_per_tick":8,"max_concurrency":1,"lease_seconds":60},"stages":[{"key":"probe","name":"Transport probe"}],"steps":steps}}),
    );
    let published = command("workflow.publish", draft["id"].as_str(), json!({}));
    let defaults = w
        .dispatch("query.configuration_defaults", json!({}))
        .unwrap();
    command(
        "project.configuration.update",
        Some(&project),
        json!({"workflow_ref":published["id"],"role_agent_bindings":defaults["role_agent_bindings"],"harness_permission_ref":defaults["harness_permission_ref"]}),
    );
    let run = command(
        "workflow.start",
        Some(&project),
        json!({"workflow_ref":published["id"],"orchestration":{"mode":"rust"}}),
    );
    let started = Instant::now();
    let mut ticks = vec![];
    loop {
        let trace = performance::Trace::start();
        let result = w.tick(|| false);
        let measurement = trace.finish();
        assert!(
            measurement["spans"]["runtime.evaluate_workflows"].is_null(),
            "workflow evaluation runs in the bounded background worker, never the scheduler polling thread"
        );
        ticks.push(json!({"elapsed_ms":started.elapsed().as_secs_f64()*1000.0,"result":result.as_ref().err(),"measurement":measurement}));
        std::fs::write(
            output.join("ticks.json"),
            serde_json::to_vec(&ticks).unwrap(),
        )
        .unwrap();
        result.unwrap();
        let state = e.store.snapshot().unwrap();
        let row = state.configuration(run["id"].as_str().unwrap()).unwrap();
        std::fs::write(
            output.join("workflow.json"),
            serde_json::to_vec_pretty(&row).unwrap(),
        )
        .unwrap();
        assert!(
            !row.data["steps"].as_array().unwrap().iter().any(|s| [
                "failed",
                "needs_reconciliation",
                "cancelled"
            ]
            .contains(&s["status"].as_str().unwrap_or(""))),
            "{}",
            row.data["steps"]
        );
        if row.data["status"] == "completed" {
            let calls: Vec<_> = state
                .governance
                .iter()
                .filter(|r| r.r#type == "platform_call")
                .collect();
            assert_eq!(calls.len(), 8);
            assert!(calls.iter().all(|r| r.data["status"] == "succeeded"));
            assert!(state.turns.is_empty(), "no model turns in this scene");
            std::fs::write(
                output.join("calls.json"),
                serde_json::to_vec_pretty(&calls).unwrap(),
            )
            .unwrap();
            let decisions: Vec<_> = state
                .governance
                .iter()
                .filter(|r| r.r#type == "workflow_decision")
                .collect();
            assert!(
                decisions
                    .iter()
                    .filter(|r| ["dispatch", "accept", "complete"]
                        .contains(&r.data["operation"].as_str().unwrap_or("")))
                    .all(|r| r.data["actor"]["kind"] == "driver")
            );
            std::fs::write(
                output.join("decisions.json"),
                serde_json::to_vec_pretty(&decisions).unwrap(),
            )
            .unwrap();
            break;
        }
        assert!(
            started.elapsed() < Duration::from_secs(180),
            "actual workflow deadline"
        );
        std::thread::sleep(Duration::from_millis(75));
    }
    println!(
        "actual workflow completed in {:.3}s; {} ticks; evidence {}",
        started.elapsed().as_secs_f64(),
        ticks.len(),
        output.display()
    );
}
