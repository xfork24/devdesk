use devdesk_adapters::*;
use devdesk_core::*;
use devdesk_domain::*;
use serde_json::{Value, json};
use std::{path::Path, sync::Arc};
struct Immediate;
impl HarnessPort for Immediate {
    fn reconcile(&self, r: &Run) -> Result<Option<Value>> {
        SimHarness.reconcile(r)
    }
    fn configure(&self, node: &str, scenario: &str) -> String {
        SimHarness.configure(node, scenario)
    }
    fn poll(&self, r: &Run, input: &Value, now: u64) -> Result<Option<Value>> {
        SimHarness.poll(r, input, now + 10_000)
    }
}
fn engine(path: &Path) -> Engine {
    engine_with_harness(path, Arc::new(Immediate))
}
fn engine_with_harness(path: &Path, harness: Arc<dyn HarnessPort>) -> Engine {
    let store = Arc::new(SqliteStore::open(path).unwrap());
    let artifacts = Arc::new(FileArtifacts::new(path.to_path_buf()));
    let simulated = devdesk_adapters::simulation::adapters();
    let e = Engine {
        store,
        artifacts,
        harness,
        git: Arc::new(SimGit),
        tests: simulated.tests,
        sandbox: Arc::new(SimSandbox),
        pull_requests: None,
        simulation: simulated.simulation,
    };
    e.bootstrap().unwrap();
    e
}
fn revision(e: &Engine, target: &str) -> Option<u64> {
    let s = e.store.snapshot().unwrap();
    s.projects
        .iter()
        .find(|x| x.id == target)
        .map(|x| x.revision)
        .or_else(|| {
            s.features
                .iter()
                .find(|x| x.id == target)
                .map(|x| x.revision)
        })
        .or_else(|| s.tasks.iter().find(|x| x.id == target).map(|x| x.revision))
        .or_else(|| {
            s.attempts
                .iter()
                .find(|x| x.id == target)
                .map(|x| x.revision)
        })
        .or_else(|| s.merges.iter().find(|x| x.id == target).map(|x| x.revision))
        .or_else(|| {
            s.contexts
                .iter()
                .find(|x| x.id == target)
                .map(|x| x.revision)
        })
        .or_else(|| {
            s.effects
                .iter()
                .find(|x| x.id == target)
                .map(|x| x.revision)
        })
}
fn cmd(e: &Engine, m: &str, t: Option<&str>, p: Value) -> Result<Value> {
    e.execute(
        m,
        Command {
            command_id: id(),
            target: t.map(str::to_owned),
            expected_revision: t.and_then(|t| revision(e, t)),
            payload: p,
        },
    )
}
fn call(e: &Engine, m: &str, t: &str, p: Value) -> Value {
    let result = cmd(e, m, if t.is_empty() { None } else { Some(t) }, p)
        .unwrap_or_else(|err| panic!("{m} {t}: {err}"));
    if m == "merge.apply" {
        e.tick(4).unwrap();
    }
    result
}
fn result_id(v: Value) -> String {
    v["id"].as_str().unwrap().into()
}
fn tick(e: &Engine) {
    for _ in 0..18 {
        e.tick(4).unwrap();
    }
}
fn new_feature(e: &Engine) -> String {
    let pid = result_id(call(e, "project.create", "", json!({"title":"测试项目"})));
    result_id(call(
        e,
        "feature.create",
        "",
        json!({"project_id":pid,"title":"导出功能"}),
    ))
}
fn review(e: &Engine, d: &str) {
    call(
        e,
        "review.submit",
        d,
        json!({"decision":"pass","reason":"检查冻结版本及证据"}),
    );
}
fn planning(e: &Engine, fid: &str) -> String {
    let aid = result_id(call(e, "attempt.start", fid, json!({"phase":"planning"})));
    tick(e);
    for kind in ["requirement", "plan"] {
        let did = result_id(call(e, "delivery.submit", &aid, json!({"kind":kind})));
        review(e, &did);
        tick(e);
    }
    assert_eq!(
        e.store.snapshot().unwrap().feature(fid).unwrap().status,
        Status::Ready
    );
    aid
}
fn task(e: &Engine, fid: &str, title: &str) -> String {
    result_id(call(e, "task.create", fid, json!({"title":title})))
}
fn implementation(e: &Engine, tid: &str, scenario: &str) -> (String, String) {
    let aid = result_id(call(e, "attempt.start", tid, json!({"scenario":scenario})));
    tick(e);
    let did = result_id(call(e, "delivery.submit", &aid, json!({})));
    (aid, did)
}
fn merge(e: &Engine, owner: &str) -> String {
    let mid = result_id(call(e, "merge.request", owner, json!({})));
    tick(e);
    let d = e
        .store
        .snapshot()
        .unwrap()
        .deliveries
        .iter()
        .find(|d| d.merge_id.as_deref() == Some(&mid))
        .unwrap()
        .id
        .clone();
    review(e, &d);
    mid
}
#[test]
fn a1_full_loop_two_tasks_closure_and_reports() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    let fid = new_feature(&e);
    planning(&e, &fid);
    for name in ["接口", "错误处理"] {
        let tid = task(&e, &fid, name);
        let (_, did) = implementation(&e, &tid, "success");
        review(&e, &did);
        let mid = merge(&e, &tid);
        call(&e, "merge.apply", &mid, json!({}));
    }
    let aid = result_id(call(&e, "attempt.start", &fid, json!({"phase":"closure"})));
    tick(&e);
    let did = result_id(call(&e, "delivery.submit", &aid, json!({})));
    review(&e, &did);
    let mid = merge(&e, &fid);
    call(&e, "merge.apply", &mid, json!({}));
    let s = e.store.snapshot().unwrap();
    assert_eq!(s.feature(&fid).unwrap().status, Status::Completed);
    assert!(s.resources.iter().all(|r| r.status == Status::Released));
    assert!(
        s.snapshots
            .iter()
            .any(|x| x.task_id.is_none() && x.kind == "final")
    );
    for a in &s.artifacts {
        assert!(!e.artifacts.read(a).unwrap().is_empty());
    }
    assert!(
        cmd(
            &e,
            "document.revise",
            Some(&fid),
            json!({"kind":"requirement","body":"变更"})
        )
        .is_err()
    );
}
#[test]
fn a2_parallel_runs_and_quotas_are_real() {
    let d = tempfile::tempdir().unwrap();
    let e = engine(d.path());
    let f = new_feature(&e);
    planning(&e, &f);
    let a = task(&e, &f, "A");
    let b = task(&e, &f, "B");
    call(&e, "attempt.start", &a, json!({"scenario":"hold"}));
    call(&e, "attempt.start", &b, json!({"scenario":"hold"}));
    e.tick(4).unwrap();
    let s = e.store.snapshot().unwrap();
    let running = s
        .runs
        .iter()
        .filter(|r| r.status == Status::Running)
        .collect::<Vec<_>>();
    assert_eq!(running.len(), 2);
    assert_ne!(running[0].attempt_id, running[1].attempt_id);
    assert_ne!(running[0].session_id, running[1].session_id);
    let old = running[0].id.clone();
    call(&e, "run.cancel", &old, json!({}));
    assert_eq!(
        e.store
            .snapshot()
            .unwrap()
            .resources
            .iter()
            .find(|r| r.run_id == old)
            .unwrap()
            .status,
        Status::Active
    );
    e.tick(4).unwrap();
    assert_eq!(
        e.store
            .snapshot()
            .unwrap()
            .resources
            .iter()
            .find(|r| r.run_id == old)
            .unwrap()
            .status,
        Status::Released
    );
}
#[test]
fn a3_technical_retry_retains_failure() {
    let d = tempfile::tempdir().unwrap();
    let e = engine(d.path());
    let f = new_feature(&e);
    planning(&e, &f);
    let t = task(&e, &f, "实现");
    let aid = result_id(call(
        &e,
        "attempt.start",
        &t,
        json!({"scenario":"technical_failure"}),
    ));
    tick(&e);
    let old = e
        .store
        .snapshot()
        .unwrap()
        .runs
        .iter()
        .find(|r| r.attempt_id.as_deref() == Some(&aid) && r.status == Status::Failed)
        .unwrap()
        .clone();
    let rid = result_id(call(&e, "run.retry", &old.id, json!({})));
    tick(&e);
    let s = e.store.snapshot().unwrap();
    assert_eq!(
        s.runs.iter().find(|r| r.id == old.id).unwrap().status,
        Status::Failed
    );
    assert_eq!(
        s.runs
            .iter()
            .find(|r| r.id == rid)
            .unwrap()
            .retry_of
            .as_ref(),
        Some(&old.id)
    );
    assert_eq!(s.attempt(&aid).unwrap().status, Status::WaitingReview);
}
#[test]
fn a4_rejection_requires_new_attempt_and_new_source() {
    let d = tempfile::tempdir().unwrap();
    let e = engine(d.path());
    let f = new_feature(&e);
    planning(&e, &f);
    let t = task(&e, &f, "实现");
    let (a1, d1) = implementation(&e, &t, "success");
    call(
        &e,
        "review.submit",
        &d1,
        json!({"decision":"changes_requested","reason":"需要修改"}),
    );
    assert!(
        cmd(
            &e,
            "review.submit",
            Some(&d1),
            json!({"decision":"pass","reason":"不能复活"})
        )
        .is_err()
    );
    let (a2, d2) = implementation(&e, &t, "success");
    assert_ne!(a1, a2);
    let s = e.store.snapshot().unwrap();
    assert_ne!(
        s.delivery(&d1).unwrap().candidate,
        s.delivery(&d2).unwrap().candidate
    );
    review(&e, &d2);
}
#[test]
fn a5_input_change_blocks_development_until_replanning() {
    let d = tempfile::tempdir().unwrap();
    let e = engine(d.path());
    let f = new_feature(&e);
    planning(&e, &f);
    let t = task(&e, &f, "实现");
    let (_, did) = implementation(&e, &t, "success");
    review(&e, &did);
    call(
        &e,
        "document.revise",
        &f,
        json!({"kind":"requirement","body":"## 新需求\n错误路径必须可观察"}),
    );
    assert!(e.store.snapshot().unwrap().delivery(&did).unwrap().stale);
    assert!(cmd(&e, "task.create", Some(&f), json!({"title":"非法"})).is_err());
    assert!(cmd(&e, "attempt.start", Some(&t), json!({})).is_err());
    assert!(
        cmd(
            &e,
            "override.accept",
            Some(&did),
            json!({"reason":"不能豁免 stale"})
        )
        .is_err()
    );
    planning(&e, &f);
    assert!(cmd(&e, "attempt.start", Some(&t), json!({})).is_err());
    call(&e, "task.replan", &t, json!({}));
    assert!(cmd(&e, "attempt.start", Some(&t), json!({})).is_ok());
}
#[test]
fn a6_target_change_invalidates_candidate_and_review() {
    let d = tempfile::tempdir().unwrap();
    let e = engine(d.path());
    let f = new_feature(&e);
    planning(&e, &f);
    let a = task(&e, &f, "A");
    let b = task(&e, &f, "B");
    for t in [&a, &b] {
        let (_, did) = implementation(&e, t, "success");
        review(&e, &did);
    }
    let ma = merge(&e, &a);
    let mb = merge(&e, &b);
    call(&e, "merge.apply", &ma, json!({}));
    assert!(cmd(&e, "merge.apply", Some(&mb), json!({})).is_err());
    let mb2 = merge(&e, &b);
    call(&e, "merge.apply", &mb2, json!({}));
    assert_eq!(
        e.store
            .snapshot()
            .unwrap()
            .merges
            .iter()
            .filter(|m| m.status == Status::Completed)
            .count(),
        2
    );
}
#[test]
fn a7_spec_upgrade_changes_checks_without_rewriting_locks() {
    let d = tempfile::tempdir().unwrap();
    let e = engine(d.path());
    let f = new_feature(&e);
    let old = planning(&e, &f);
    let s = e.store.snapshot().unwrap();
    let mut binding = s.feature(&f).unwrap().binding.clone();
    let standard = s
        .definitions
        .iter()
        .find(|d| d.id == binding.standard)
        .unwrap();
    let v2 = s
        .definitions
        .iter()
        .find(|d| d.family == "spec" && d.version == 2)
        .unwrap();
    let slots = standard
        .slots
        .iter()
        .map(|slot| json!({"purpose":slot.purpose,"spec":v2.id}))
        .collect::<Vec<_>>();
    let sid = result_id(call(
        &e,
        "definition.revise",
        &standard.id,
        json!({"body":"升级规范","slots":slots}),
    ));
    binding.standard = sid;
    call(
        &e,
        "binding.update",
        &f,
        serde_json::to_value(&binding).unwrap(),
    );
    let t = task(&e, &f, "升级后任务");
    let (_, did) = implementation(&e, &t, "success");
    review(&e, &did);
    let s = e.store.snapshot().unwrap();
    assert_ne!(s.attempt(&old).unwrap().binding.standard, binding.standard);
    assert!(
        s.reviews
            .iter()
            .find(|r| r.delivery_id == did)
            .unwrap()
            .checks
            .iter()
            .any(|c| c.key.contains("failure_paths"))
    );
}
#[test]
fn a8_quality_override_keeps_test_failure_and_reason() {
    let d = tempfile::tempdir().unwrap();
    let e = engine(d.path());
    let f = new_feature(&e);
    planning(&e, &f);
    let t = task(&e, &f, "质量例外");
    let (_, did) = implementation(&e, &t, "assertion_failure");
    review(&e, &did);
    let s = e.store.snapshot().unwrap();
    assert!(!s.accepted(s.delivery(&did).unwrap()));
    let failed = s
        .runs
        .iter()
        .find(|r| r.node == "green" && r.status == Status::Failed)
        .unwrap()
        .id
        .clone();
    call(
        &e,
        "override.accept",
        &did,
        json!({"reason":"已知质量问题，明确接受","items":["review",format!("test:{failed}")]}),
    );
    let s = e.store.snapshot().unwrap();
    assert!(s.accepted(s.delivery(&did).unwrap()));
    assert_eq!(
        s.runs.iter().find(|r| r.id == failed).unwrap().status,
        Status::Failed
    );
    assert_eq!(s.task(&t).unwrap().status, Status::Accepted);
}
#[test]
fn a9_restart_idempotence_lock_and_artifact_integrity() {
    let d = tempfile::tempdir().unwrap();
    let e = engine(d.path());
    assert!(SqliteStore::open(d.path()).is_err());
    let cid = id();
    let c = Command {
        command_id: cid.clone(),
        target: None,
        expected_revision: None,
        payload: json!({"title":"幂等"}),
    };
    let a = e.execute("project.create", c.clone()).unwrap();
    assert_eq!(a, e.execute("project.create", c.clone()).unwrap());
    let mut bad = c;
    bad.payload = json!({"title":"其他"});
    assert!(e.execute("project.create", bad).is_err());
    let f = new_feature(&e);
    call(
        &e,
        "attempt.start",
        &f,
        json!({"phase":"planning","scenario":"hold"}),
    );
    e.tick(4).unwrap();
    drop(e);
    let e = engine(d.path());
    let s = e.store.snapshot().unwrap();
    assert!(s.runs.iter().any(|r| r.status == Status::Interrupted));
    let rid = s
        .runs
        .iter()
        .find(|r| r.status == Status::Interrupted)
        .unwrap()
        .id
        .clone();
    call(&e, "run.retry", &rid, json!({}));
}
#[test]
fn a10_context_updates_do_not_mutate_snapshot() {
    let d = tempfile::tempdir().unwrap();
    let e = engine(d.path());
    let f = new_feature(&e);
    planning(&e, &f);
    let t = task(&e, &f, "独立上下文");
    let s = e.store.snapshot().unwrap();
    let snap = s.snapshots[0].clone();
    let fc = s
        .contexts
        .iter()
        .find(|c| c.owner_id == f)
        .unwrap()
        .id
        .clone();
    call(&e, "context.update", &fc, json!({"content":"新的内容"}));
    let s = e.store.snapshot().unwrap();
    assert_eq!(s.snapshots[0].content, snap.content);
    assert_ne!(
        s.contexts.iter().find(|c| c.owner_id == t).unwrap().content,
        "新的内容"
    );
}
#[test]
fn a11_hard_tdd_constraints_cannot_be_overridden() {
    let d = tempfile::tempdir().unwrap();
    let e = engine(d.path());
    let f = new_feature(&e);
    planning(&e, &f);
    let t = task(&e, &f, "约束");
    let (_, did) = implementation(&e, &t, "success");
    let s = e.store.snapshot().unwrap();
    let delivery = s.delivery(&did).unwrap().clone();
    assert!(s.validate_evidence(&delivery).is_ok());
    for mutation in 0..4 {
        let mut state = s.clone();
        let red = state.runs.iter_mut().find(|r| r.node == "red").unwrap();
        match mutation {
            0 => red.result.as_mut().unwrap()["expected_failure"] = json!(false),
            1 => red.finished = Some(u64::MAX),
            2 => red.result.as_mut().unwrap()["candidate"] = json!("sim-rev-wrong"),
            _ => red.result.as_mut().unwrap()["test_definitions"] = json!(["other"]),
        }
        assert!(state.validate_evidence(&delivery).is_err());
    }
    let mut live = s.clone();
    live.projects[0].execution_mode = "live".into();
    assert!(live.validate_evidence(&delivery).is_err());
}
#[test]
fn malformed_adapter_result_is_a_failure_not_success() {
    let d = tempfile::tempdir().unwrap();
    let e = engine(d.path());
    let f = new_feature(&e);
    let aid = result_id(call(
        &e,
        "attempt.start",
        &f,
        json!({"phase":"planning","scenario":"missing_fields"}),
    ));
    tick(&e);
    let s = e.store.snapshot().unwrap();
    assert_eq!(s.attempt(&aid).unwrap().status, Status::Failed);
    assert!(cmd(&e, "delivery.submit", Some(&aid), json!({})).is_err());
}
#[test]
fn pause_stops_dispatch_and_cleanup_failure_is_retained() {
    let d = tempfile::tempdir().unwrap();
    let e = engine(d.path());
    let f = new_feature(&e);
    let aid = result_id(call(
        &e,
        "attempt.start",
        &f,
        json!({"phase":"planning","scenario":"cleanup_failure"}),
    ));
    call(&e, "attempt.pause", &aid, json!({"paused":true}));
    e.tick(4).unwrap();
    assert!(
        e.store
            .snapshot()
            .unwrap()
            .runs
            .iter()
            .all(|r| r.status == Status::Queued)
    );
    call(&e, "attempt.pause", &aid, json!({"paused":false}));
    tick(&e);
    assert!(
        e.store
            .snapshot()
            .unwrap()
            .resources
            .iter()
            .any(|r| r.status == Status::CleanupFailed)
    );
}
#[test]
fn revision_conflicts_and_pagination_are_enforced() {
    let d = tempfile::tempdir().unwrap();
    let e = engine(d.path());
    let f = new_feature(&e);
    assert!(
        e.execute(
            "attempt.start",
            Command {
                command_id: id(),
                target: Some(f.clone()),
                expected_revision: Some(999),
                payload: json!({"phase":"planning"})
            }
        )
        .is_err()
    );
    let value = e.query("query.history", &json!({"limit":1}), 4).unwrap();
    assert_eq!(value["events"].as_array().unwrap().len(), 1);
}

#[test]
fn revised_source_and_rejected_merge_allow_new_candidates() {
    let d = tempfile::tempdir().unwrap();
    let e = engine(d.path());
    let f = new_feature(&e);
    planning(&e, &f);
    let t = task(&e, &f, "更新源");
    let (_, d1) = implementation(&e, &t, "success");
    review(&e, &d1);
    let old = merge(&e, &t);
    let (_, d2) = implementation(&e, &t, "success");
    review(&e, &d2);
    assert!(cmd(&e, "merge.apply", Some(&old), json!({})).is_err());
    let next = result_id(call(&e, "merge.request", &t, json!({})));
    tick(&e);
    let did = e
        .store
        .snapshot()
        .unwrap()
        .deliveries
        .iter()
        .find(|d| d.merge_id.as_deref() == Some(&next))
        .unwrap()
        .id
        .clone();
    call(
        &e,
        "review.submit",
        &did,
        json!({"decision":"changes_requested","reason":"候选需重建"}),
    );
    let final_merge = merge(&e, &t);
    call(&e, "merge.apply", &final_merge, json!({}));
}
#[test]
fn cancelled_attempt_cannot_be_revived_by_review_or_override() {
    let d = tempfile::tempdir().unwrap();
    let e = engine(d.path());
    let f = new_feature(&e);
    planning(&e, &f);
    let t = task(&e, &f, "取消");
    let (aid, did) = implementation(&e, &t, "success");
    call(&e, "attempt.cancel", &aid, json!({}));
    assert!(
        cmd(
            &e,
            "review.submit",
            Some(&did),
            json!({"decision":"pass","reason":"不可复活"})
        )
        .is_err()
    );
    assert!(
        cmd(
            &e,
            "override.accept",
            Some(&did),
            json!({"reason":"不可复活"})
        )
        .is_err()
    );
    assert_eq!(
        e.store.snapshot().unwrap().attempt(&aid).unwrap().status,
        Status::Cancelled
    );
}
#[test]
fn independent_agent_review_receives_frozen_delivery_for_attempt_and_merge() {
    let d = tempfile::tempdir().unwrap();
    let e = engine(d.path());
    let f = new_feature(&e);
    planning(&e, &f);
    let t = task(&e, &f, "Agent 评审");
    let (_, did) = implementation(&e, &t, "success");
    call(&e, "review.request", &did, json!({}));
    tick(&e);
    let s = e.store.snapshot().unwrap();
    assert!(s.accepted(s.delivery(&did).unwrap()));
    let author = s
        .attempt(s.delivery(&did).unwrap().attempt_id.as_deref().unwrap())
        .unwrap()
        .binding
        .agent
        .clone();
    let rr = s
        .runs
        .iter()
        .find(|r| r.node == format!("review:{did}"))
        .unwrap();
    assert_ne!(
        author,
        s.sessions
            .iter()
            .find(|x| x.id == rr.session_id)
            .unwrap()
            .agent
    );
    let m = result_id(call(&e, "merge.request", &t, json!({})));
    tick(&e);
    let md = e
        .store
        .snapshot()
        .unwrap()
        .deliveries
        .iter()
        .find(|d| d.merge_id.as_deref() == Some(&m))
        .unwrap()
        .id
        .clone();
    call(&e, "review.request", &md, json!({}));
    tick(&e);
    assert!(
        e.store
            .snapshot()
            .unwrap()
            .accepted(e.store.snapshot().unwrap().delivery(&md).unwrap())
    );
}
#[test]
fn reviewer_restart_does_not_change_accepted_attempt() {
    let d = tempfile::tempdir().unwrap();
    let e = engine(d.path());
    let f = new_feature(&e);
    planning(&e, &f);
    let t = task(&e, &f, "后置评审");
    let (aid, did) = implementation(&e, &t, "success");
    call(&e, "review.request", &did, json!({"scenario":"hold"}));
    e.tick(4).unwrap();
    review(&e, &did);
    drop(e);
    let e = engine(d.path());
    assert_eq!(
        e.store.snapshot().unwrap().attempt(&aid).unwrap().status,
        Status::Accepted
    );
}
#[test]
fn merge_command_replay_after_restart_does_not_advance_twice() {
    let d = tempfile::tempdir().unwrap();
    let e = engine(d.path());
    let f = new_feature(&e);
    planning(&e, &f);
    let t = task(&e, &f, "幂等合入");
    let (_, did) = implementation(&e, &t, "success");
    review(&e, &did);
    let m = merge(&e, &t);
    let command = Command {
        command_id: id(),
        target: Some(m.clone()),
        expected_revision: revision(&e, &m),
        payload: json!({}),
    };
    let first = e.execute("merge.apply", command.clone()).unwrap();
    e.tick(4).unwrap();
    let original = e.store.snapshot().unwrap().heads;
    drop(e);
    let e = engine(d.path());
    assert_eq!(first, e.execute("merge.apply", command).unwrap());
    assert_eq!(
        serde_json::to_value(original).unwrap(),
        serde_json::to_value(e.store.snapshot().unwrap().heads).unwrap()
    );
    assert_eq!(
        e.store
            .snapshot()
            .unwrap()
            .merges
            .iter()
            .filter(|m| m.status == Status::Completed)
            .count(),
        1
    );
}
#[test]
fn retrying_quality_failure_before_delivery_reexecutes_downstream() {
    let d = tempfile::tempdir().unwrap();
    let e = engine(d.path());
    let f = new_feature(&e);
    planning(&e, &f);
    let t = task(&e, &f, "下游重验");
    let a = result_id(call(
        &e,
        "attempt.start",
        &t,
        json!({"scenario":"assertion_failure"}),
    ));
    tick(&e);
    let old = e
        .store
        .snapshot()
        .unwrap()
        .runs
        .iter()
        .find(|r| r.attempt_id.as_deref() == Some(&a) && r.node == "green")
        .unwrap()
        .id
        .clone();
    call(&e, "run.retry", &old, json!({}));
    assert!(cmd(&e, "run.retry", Some(&old), json!({})).is_err());
    tick(&e);
    let did = result_id(call(&e, "delivery.submit", &a, json!({})));
    let s = e.store.snapshot().unwrap();
    s.validate_evidence(s.delivery(&did).unwrap()).unwrap();
    assert_eq!(
        s.runs
            .iter()
            .filter(|r| r.attempt_id.as_deref() == Some(&a) && r.node == "verify")
            .count(),
        2
    );
}
#[cfg(unix)]
#[test]
fn artifact_symlinks_cannot_escape_dataset() {
    use std::os::unix::fs::symlink;
    let d = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let e = engine(d.path());
    std::fs::remove_dir(d.path().join("artifacts")).unwrap();
    symlink(outside.path(), d.path().join("artifacts")).unwrap();
    assert!(
        e.artifacts
            .seal("feature", None, "documents", "test")
            .is_err()
    );
    assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 0);
    drop(e);
    assert!(SqliteStore::open(d.path()).is_err());
}
#[test]
fn duplicate_polls_do_not_duplicate_deliveries_or_reviews() {
    let d = tempfile::tempdir().unwrap();
    let e = engine(d.path());
    let f = new_feature(&e);
    planning(&e, &f);
    let t = task(&e, &f, "重复输出");
    let (_, did) = implementation(&e, &t, "success");
    call(&e, "review.request", &did, json!({}));
    tick(&e);
    let s = e.store.snapshot().unwrap();
    tick(&e);
    let after = e.store.snapshot().unwrap();
    assert_eq!(s.reviews.len(), after.reviews.len());
    assert_eq!(s.deliveries.len(), after.deliveries.len());
}
#[test]
fn completed_delivery_override_does_not_reopen_task() {
    let d = tempfile::tempdir().unwrap();
    let e = engine(d.path());
    let f = new_feature(&e);
    planning(&e, &f);
    let t = task(&e, &f, "冻结");
    let (_, did) = implementation(&e, &t, "success");
    review(&e, &did);
    let m = merge(&e, &t);
    call(&e, "merge.apply", &m, json!({}));
    let snapshots = e.store.snapshot().unwrap().snapshots.len();
    assert!(
        cmd(
            &e,
            "override.accept",
            Some(&did),
            json!({"items":["review"],"reason":"不能重开"})
        )
        .is_err()
    );
    assert_eq!(
        e.store.snapshot().unwrap().task(&t).unwrap().status,
        Status::Completed
    );
    assert_eq!(e.store.snapshot().unwrap().snapshots.len(), snapshots);
}
#[test]
fn invalidating_inflight_merge_does_not_poison_scheduler() {
    for revise in [false, true] {
        let d = tempfile::tempdir().unwrap();
        let e = engine(d.path());
        let f = new_feature(&e);
        planning(&e, &f);
        let t = task(&e, &f, "在途失效");
        let (_, did) = implementation(&e, &t, "success");
        review(&e, &did);
        let m = result_id(call(&e, "merge.request", &t, json!({})));
        e.tick(4).unwrap();
        if revise {
            call(
                &e,
                "document.revise",
                &f,
                json!({"kind":"requirement","body":"## 变化\n失败路径"}),
            );
            planning(&e, &f);
        } else {
            call(&e, "attempt.start", &t, json!({}));
        }
        tick(&e);
        let s = e.store.snapshot().unwrap();
        assert_eq!(s.merge(&m).unwrap().status, Status::Interrupted);
        assert!(
            !s.runs
                .iter()
                .any(|r| r.merge_id.as_deref() == Some(&m) && r.status == Status::Running)
        );
    }
}
#[cfg(unix)]
#[test]
fn artifact_read_rejects_symlink_replacement() {
    use std::os::unix::fs::symlink;
    let d = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    let e = engine(d.path());
    let a = e
        .artifacts
        .seal("feature", None, "documents", "original")
        .unwrap();
    std::fs::rename(d.path().join("artifacts"), out.path().join("moved")).unwrap();
    symlink(out.path().join("moved"), d.path().join("artifacts")).unwrap();
    assert!(e.artifacts.read(&a).is_err());
}
#[test]
fn detail_pages_are_reachable_and_do_not_page_projects() {
    let d = tempfile::tempdir().unwrap();
    let e = engine(d.path());
    let f = new_feature(&e);
    planning(&e, &f);
    for n in 0..51 {
        task(&e, &f, &format!("任务 {n}"));
    }
    let page = e
        .query(
            "query.workspace",
            &json!({"feature_id":f,"offset":50,"detail_pages":{"tasks":1}}),
            4,
        )
        .unwrap();
    assert_eq!(page["projects"].as_array().unwrap().len(), 1);
    assert_eq!(page["detail"]["tasks"].as_array().unwrap().len(), 1);
    assert_eq!(page["detail"]["tasks"][0]["title"], "任务 50");
    assert_eq!(page["detail"]["totals"]["tasks"], 51);
}
#[test]
fn small_sample_pagination_and_event_query() {
    let d = tempfile::tempdir().unwrap();
    let e = engine(d.path());
    let b = e.store.snapshot().unwrap();
    let binding = BindingLock {
        agent: b
            .definitions
            .iter()
            .find(|d| d.family == "agent")
            .unwrap()
            .id
            .clone(),
        standard: b
            .definitions
            .iter()
            .find(|d| d.family == "standard")
            .unwrap()
            .id
            .clone(),
        skills: vec![],
        harness: "sim-harness-v1".into(),
        sandbox: "sim-sandbox-v1".into(),
    };
    e.store
        .transact(None, &mut |s| {
            for pn in 0..10 {
                let pid = id();
                s.projects.push(Project {
                    permission_overrides: None,
                    runtime_preferences: None,
                    workspace_definition: None,
                    id: pid.clone(),
                    title: format!("Project {pn}"),
                    workspace: String::new(),
                    execution_mode: "simulation".into(),
                    binding: binding.clone(),
                    concurrency: 2,
                    revision: 1,
                });
                for fn_ in 0..10 {
                    let fid = id();
                    let sid = id();
                    s.features.push(Feature {
                        source_requirement: None,
                        id: fid.clone(),
                        project_id: pid.clone(),
                        title: format!("Feature {fn_}"),
                        status: Status::Ready,
                        revision: 1,
                        input_version: 1,
                        binding: binding.clone(),
                        predecessor: None,
                    });
                    s.snapshots.push(Snapshot {
                        execution_instances: vec![],
                        runtime_bindings: vec![],
                        project_id: None,
                        capture: SnapshotCapture::default(),
                        id: sid.clone(),
                        feature_id: Some(fid.clone()),
                        task_id: None,
                        kind: "initial".into(),
                        content: String::new(),
                        binding: binding.clone(),
                        documents: vec![],
                        delivery_id: None,
                        inherited_from: None,
                    });
                    for tn in 0..10 {
                        let tid = id();
                        s.tasks.push(Task {
                            scope: TaskScope::default(),
                            id: tid.clone(),
                            feature_id: fid.clone(),
                            title: format!("Task {tn}"),
                            status: Status::Ready,
                            revision: 1,
                            binding: binding.clone(),
                            inherited_snapshot: sid.clone(),
                            dependencies: vec![],
                            worktree: format!("logical/{tid}"),
                        });
                    }
                }
            }
            Ok(json!({}))
        })
        .unwrap();
    let start = std::time::Instant::now();
    let result = e.query("query.workspace", &json!({"limit":20}), 4).unwrap();
    assert_eq!(result["total_features"], 100);
    assert_eq!(result["features"].as_array().unwrap().len(), 20);
    let fid = result["features"][0]["id"].as_str().unwrap();
    let detail = e
        .query("query.workspace", &json!({"feature_id":fid,"limit":5}), 4)
        .unwrap();
    assert_eq!(detail["detail"]["tasks"].as_array().unwrap().len(), 5);
    e.query("query.events", &json!({"cursor":0,"limit":10}), 4)
        .unwrap();
    println!(
        "small sample: 10 Projects / 100 Features / 1000 Tasks; list + detail + event query = {:?}",
        start.elapsed()
    );
}
#[test]
fn alternative_spec_is_an_actual_replacement() {
    let d = tempfile::tempdir().unwrap();
    let e = engine(d.path());
    let f = new_feature(&e);
    planning(&e, &f);
    let s = e.store.snapshot().unwrap();
    let mut b = s.feature(&f).unwrap().binding.clone();
    let standard = s.definitions.iter().find(|d| d.id == b.standard).unwrap();
    let alt = s
        .definitions
        .iter()
        .find(|d| d.name == "Alternative Design Spec")
        .unwrap();
    let slots = standard
        .slots
        .iter()
        .map(|slot| json!({"purpose":slot.purpose,"spec":alt.id}))
        .collect::<Vec<_>>();
    b.standard = result_id(call(
        &e,
        "definition.revise",
        &standard.id,
        json!({"body":"替代规则","slots":slots}),
    ));
    let t = task(&e, &f, "替代规范");
    call(&e, "binding.update", &t, serde_json::to_value(&b).unwrap());
    let (_, did) = implementation(&e, &t, "success");
    review(&e, &did);
    let s = e.store.snapshot().unwrap();
    assert!(
        s.reviews
            .iter()
            .find(|r| r.delivery_id == did)
            .unwrap()
            .checks
            .iter()
            .any(|c| c.key.contains("Alternative Design Spec:v1:sections"))
    );
    assert_ne!(s.feature(&f).unwrap().binding.standard, b.standard);
}
#[test]
fn logical_write_permit_is_enforced_independently_of_dag() {
    let d = tempfile::tempdir().unwrap();
    let e = engine(d.path());
    let f = new_feature(&e);
    planning(&e, &f);
    let t = task(&e, &f, "写许可");
    let a = result_id(call(&e, "attempt.start", &t, json!({"scenario":"hold"})));
    e.store
        .transact(None, &mut |s| {
            let docs = s
                .runs
                .iter_mut()
                .find(|r| r.attempt_id.as_deref() == Some(&a) && r.node == "docs")
                .unwrap();
            docs.dependencies = vec!["red".into()];
            let nodes = s
                .latest_runs(Some(&a), None)
                .iter()
                .map(|r| (r.node.clone(), r.dependencies.clone()))
                .collect::<Vec<_>>();
            valid_dag(&nodes)?;
            Ok(json!({}))
        })
        .unwrap();
    e.tick(4).unwrap();
    let red = e
        .store
        .snapshot()
        .unwrap()
        .runs
        .iter()
        .find(|r| r.attempt_id.as_deref() == Some(&a) && r.node == "red")
        .unwrap()
        .id
        .clone();
    call(&e, "run.continue", &red, json!({}));
    e.tick(4).unwrap();
    let s = e.store.snapshot().unwrap();
    assert_eq!(
        s.runs
            .iter()
            .filter(|r| r.attempt_id.as_deref() == Some(&a)
                && r.status == Status::Running
                && r.writes)
            .count(),
        1
    );
    let docs = s
        .runs
        .iter()
        .find(|r| r.attempt_id.as_deref() == Some(&a) && r.node == "docs")
        .unwrap();
    assert_eq!(docs.status, Status::Queued);
    let query = e
        .query("query.workspace", &json!({"feature_id":f}), 4)
        .unwrap();
    assert!(
        query["detail"]["blocked_reasons"][&docs.id]
            .as_str()
            .unwrap()
            .contains("写许可")
    );
}

#[test]
fn hardening_document_target_cannot_bypass_revision() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    let fid = new_feature(&e);
    assert!(
        cmd(
            &e,
            "document.revise",
            None,
            json!({"feature_id":fid,"kind":"requirement","body":"x"})
        )
        .is_err()
    );
    assert!(
        e.execute(
            "document.revise",
            Command {
                command_id: id(),
                target: Some(fid),
                expected_revision: None,
                payload: json!({"kind":"requirement","body":"x"})
            }
        )
        .is_err()
    );
}
#[test]
fn hardening_reviewer_retry_preserves_full_binding() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    let f = new_feature(&e);
    planning(&e, &f);
    let t = task(&e, &f, "review");
    let (a, d) = implementation(&e, &t, "success");
    let r = result_id(call(&e, "review.request", &d, json!({"scenario":"hold"})));
    e.tick(4).unwrap();
    call(&e, "run.cancel", &r, json!({}));
    e.tick(4).unwrap();
    let rr = result_id(call(&e, "run.retry", &r, json!({})));
    let s = e.store.snapshot().unwrap();
    let old = s.runs.iter().find(|x| x.id == r).unwrap();
    let new = s.runs.iter().find(|x| x.id == rr).unwrap();
    assert_eq!(json!(old.binding), json!(new.binding));
    assert_ne!(new.binding.agent, s.attempt(&a).unwrap().binding.agent);
    tick(&e);
    assert!(
        e.store
            .snapshot()
            .unwrap()
            .accepted(e.store.snapshot().unwrap().delivery(&d).unwrap())
    );
}
#[test]
fn hardening_task_document_change_is_local() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    let f = new_feature(&e);
    planning(&e, &f);
    let a = task(&e, &f, "A");
    let b = task(&e, &f, "B");
    let (_, da) = implementation(&e, &a, "success");
    let (_, db) = implementation(&e, &b, "success");
    call(
        &e,
        "document.revise",
        &f,
        json!({"kind":"doc","task_id":a,"body":"局部说明"}),
    );
    let s = e.store.snapshot().unwrap();
    assert!(s.planning_accepted(&f));
    assert!(s.delivery(&da).unwrap().stale);
    assert!(!s.delivery(&db).unwrap().stale);
    assert!(cmd(&e, "attempt.start", Some(&a), json!({})).is_ok());
}
struct EmptySuccessfulHarness;
impl HarnessPort for EmptySuccessfulHarness {
    fn configure(&self, _: &str, _: &str) -> String {
        "success".into()
    }
    fn poll(&self, _: &Run, _: &Value, _: u64) -> Result<Option<Value>> {
        Ok(Some(json!({"status":"succeeded","origin":"simulated"})))
    }
}
#[test]
fn hardening_empty_success_cannot_accept_plan() {
    let dir = tempfile::tempdir().unwrap();
    let mut e = engine(dir.path());
    e.harness = Arc::new(EmptySuccessfulHarness);
    let f = new_feature(&e);
    call(
        &e,
        "document.revise",
        &f,
        json!({"kind":"requirement","body":"目标"}),
    );
    let a = result_id(call(&e, "attempt.start", &f, json!({"phase":"planning"})));
    tick(&e);
    let requirement = result_id(call(
        &e,
        "delivery.submit",
        &a,
        json!({"kind":"requirement"}),
    ));
    review(&e, &requirement);
    tick(&e);
    assert!(cmd(&e, "delivery.submit", Some(&a), json!({"kind":"plan"})).is_err());
}
#[test]
fn hardening_trace_scope_and_snapshot_provenance() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    let f = new_feature(&e);
    planning(&e, &f);
    let s = e.store.snapshot().unwrap();
    let snapshot = s.snapshots.last().unwrap();
    assert!(!snapshot.capture.sessions.is_empty());
    assert!(!snapshot.capture.resources.is_empty());
    assert!(cmd(&e,"task.create",Some(&f),json!({"title":"invalid","scope":{"requirement":null,"design":null,"acceptance":["missing"]}})).is_err());
    let t = task(&e, &f, "mapped");
    let s = e.store.snapshot().unwrap();
    let scope = &s.task(&t).unwrap().scope;
    s.validate_scope(&f, scope).unwrap();
    assert!(!scope.acceptance.is_empty());
}
#[test]
fn hardening_adapter_reads_committed_store_outside_transaction() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    // Previously this re-entrant read deadlocked on the SQLite connection mutex.
    e.store
        .transact(None, &mut |s| {
            let committed = e.store.snapshot()?;
            assert_eq!(committed.projects.len(), s.projects.len());
            Ok(json!({}))
        })
        .unwrap();
}

#[test]
fn hardening_requirement_review_blocks_design_dispatch() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    let f = new_feature(&e);
    let a = result_id(call(&e, "attempt.start", &f, json!({"phase":"planning"})));
    tick(&e);
    let s = e.store.snapshot().unwrap();
    assert_eq!(
        s.runs
            .iter()
            .find(|r| r.attempt_id.as_deref() == Some(&a) && r.node == "design")
            .unwrap()
            .status,
        Status::Queued
    );
    assert!(cmd(&e, "delivery.submit", Some(&a), json!({"kind":"plan"})).is_err());
    let d = result_id(call(
        &e,
        "delivery.submit",
        &a,
        json!({"kind":"requirement"}),
    ));
    review(&e, &d);
    tick(&e);
    assert_eq!(
        e.store
            .snapshot()
            .unwrap()
            .runs
            .iter()
            .find(|r| r.attempt_id.as_deref() == Some(&a) && r.node == "design")
            .unwrap()
            .status,
        Status::Succeeded
    );
}
#[test]
fn hardening_v1_migration_preserves_reviewer_binding_and_legacy_provenance() {
    let dir = tempfile::tempdir().unwrap();
    let f;
    {
        let e = engine(dir.path());
        f = new_feature(&e);
        planning(&e, &f);
    }
    // Build a genuine v1 database from its committed schema, copying only v1 columns.
    // Future schema additions must not leak into this historical migration fixture.
    let original = dir.path().join("engineering.sqlite3");
    let legacy = dir.path().join("legacy-v1.sqlite3");
    let mut conn = rusqlite::Connection::open(&legacy).unwrap();
    conn.execute_batch(include_str!("../../../sql/001_initial.sql"))
        .unwrap();
    conn.execute(
        "ATTACH DATABASE ?1 AS original",
        [original.to_string_lossy().as_ref()],
    )
    .unwrap();
    let tables: Vec<String> = conn
        .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<std::result::Result<_, _>>()
        .unwrap();
    let tx = conn.transaction().unwrap();
    tx.execute_batch("PRAGMA defer_foreign_keys=ON").unwrap();
    for table in tables {
        let columns: Vec<String> = tx
            .prepare(&format!("PRAGMA table_info({table})"))
            .unwrap()
            .query_map([], |row| row.get(1))
            .unwrap()
            .collect::<std::result::Result<_, _>>()
            .unwrap();
        let columns = columns
            .iter()
            .map(|c| format!("\"{c}\""))
            .collect::<Vec<_>>()
            .join(",");
        tx.execute_batch(&format!(
            "INSERT INTO {table}({columns}) SELECT {columns} FROM original.{table}"
        ))
        .unwrap();
    }
    tx.commit().unwrap();
    conn.execute_batch("DETACH DATABASE original").unwrap();
    drop(conn);
    std::fs::rename(&legacy, &original).unwrap();
    let e = engine(dir.path());
    let s = e.store.snapshot().unwrap();
    assert_eq!(s.feature(&f).unwrap().status, Status::Ready);
    for r in &s.runs {
        let session = s
            .sessions
            .iter()
            .find(|session| session.id == r.session_id)
            .unwrap();
        assert_eq!(r.binding.agent, session.agent);
    }
    assert!(
        s.snapshots
            .iter()
            .all(|snap| snap.capture.captured_at.is_none())
    );
    assert!(s.documents.iter().all(|doc| doc.acceptance.is_empty()));
}
#[test]
fn hardening_runtime_instances_and_archive_contract_are_queryable() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    let f = new_feature(&e);
    planning(&e, &f);
    let s = e.store.snapshot().unwrap();
    assert!(s.sessions.iter().all(|session| {
        session
            .instance
            .as_ref()
            .is_some_and(|i| i.finished.is_some() && i.harness.provider == "simulation")
    }));
    let runtimes = e.query("query.runtimes", &json!({}), 4).unwrap();
    assert_eq!(runtimes["harnesses"][0]["version"], "1");
    let preview = e
        .query("query.archive_preview", &json!({"feature_id":f}), 4)
        .unwrap();
    assert_eq!(preview["executable"], false);
}

struct CancelAckHarness(std::sync::atomic::AtomicBool);
impl HarnessPort for CancelAckHarness {
    fn configure(&self, _: &str, _: &str) -> String {
        "hold".into()
    }
    fn poll(&self, r: &Run, _: &Value, _: u64) -> Result<Option<Value>> {
        if !r.cancel_requested {
            return Ok(None);
        }
        if self.0.load(std::sync::atomic::Ordering::SeqCst) {
            Ok(Some(
                json!({"status":"cancelled","cancellation_confirmed":true}),
            ))
        } else {
            Ok(Some(json!({"status":"succeeded"})))
        }
    }
}
#[test]
fn hardening_late_success_does_not_acknowledge_cancel() {
    let dir = tempfile::tempdir().unwrap();
    let mut e = engine(dir.path());
    let harness = Arc::new(CancelAckHarness(std::sync::atomic::AtomicBool::new(false)));
    e.harness = harness.clone();
    let f = new_feature(&e);
    call(&e, "attempt.start", &f, json!({"phase":"planning"}));
    e.tick(4).unwrap();
    let r = e
        .store
        .snapshot()
        .unwrap()
        .runs
        .iter()
        .find(|r| r.status == Status::Running)
        .unwrap()
        .id
        .clone();
    call(&e, "run.cancel", &r, json!({}));
    e.tick(4).unwrap();
    assert_eq!(
        e.store
            .snapshot()
            .unwrap()
            .runs
            .iter()
            .find(|run| run.id == r)
            .unwrap()
            .status,
        Status::Running
    );
    assert!(
        e.store
            .snapshot()
            .unwrap()
            .resources
            .iter()
            .any(|res| res.run_id == r && res.status == Status::Active)
    );
    harness.0.store(true, std::sync::atomic::Ordering::SeqCst);
    e.tick(4).unwrap();
    assert_eq!(
        e.store
            .snapshot()
            .unwrap()
            .runs
            .iter()
            .find(|run| run.id == r)
            .unwrap()
            .status,
        Status::Cancelled
    );
}
struct FailingRunExport(FileArtifacts);
impl ArtifactStore for FailingRunExport {
    fn seal(&self, f: &str, t: Option<&str>, category: &str, body: &str) -> Result<Artifact> {
        if category == "runs" {
            Err(err("storage", "export unavailable"))
        } else {
            self.0.seal(f, t, category, body)
        }
    }
    fn read(&self, a: &Artifact) -> Result<Vec<u8>> {
        self.0.read(a)
    }
}
struct CountCleanup(Arc<std::sync::atomic::AtomicUsize>);
impl SandboxPort for CountCleanup {
    fn resource(&self, r: &str) -> Resource {
        SimSandbox.resource(r)
    }
    fn cleanup(&self, r: &mut Resource, fail: bool) {
        self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        SimSandbox.cleanup(r, fail);
    }
}
#[test]
fn hardening_failed_export_keeps_environment_owned() {
    let dir = tempfile::tempdir().unwrap();
    let mut e = engine(dir.path());
    let count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    e.sandbox = Arc::new(CountCleanup(count.clone()));
    e.artifacts = Arc::new(FailingRunExport(FileArtifacts::new(
        dir.path().to_path_buf(),
    )));
    let f = new_feature(&e);
    call(&e, "attempt.start", &f, json!({"phase":"planning"}));
    e.tick(4).unwrap();
    assert!(e.tick(4).is_err());
    assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 0);
    assert!(
        e.store
            .snapshot()
            .unwrap()
            .resources
            .iter()
            .all(|r| r.status == Status::Active)
    );
}

#[test]
fn hardening_replanned_task_uses_current_criteria_without_rewriting_attempt() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    let f = new_feature(&e);
    planning(&e, &f);
    let t = task(&e, &f, "existing");
    let (old, _) = implementation(&e, &t, "success");
    let old_input = e
        .store
        .snapshot()
        .unwrap()
        .attempt(&old)
        .unwrap()
        .documents
        .clone();
    call(
        &e,
        "document.revise",
        &f,
        json!({"kind":"requirement","body":"新验收","acceptance":[{"key":"CSV-EMPTY","description":"空列表包含CSV表头"}]}),
    );
    planning(&e, &f);
    call(&e, "task.replan", &t, json!({}));
    let (_, d) = implementation(&e, &t, "success");
    review(&e, &d);
    let s = e.store.snapshot().unwrap();
    assert_eq!(s.task(&t).unwrap().scope.acceptance, vec!["CSV-EMPTY"]);
    assert_eq!(s.attempt(&old).unwrap().documents, old_input);
}

struct DistinctCodeRevisions;
impl GitPort for DistinctCodeRevisions {
    fn execution_revision(&self, a: &Attempt, phase: &str) -> Result<String> {
        Ok(format!(
            "sim-rev-{}-{}",
            if phase == "red" {
                "base"
            } else {
                "implementation"
            },
            a.id
        ))
    }
    fn candidate(&self, context: &GitContext, s: &str, t: &str, o: &str) -> Result<String> {
        SimGit.candidate(context, s, t, o)
    }
    fn advance(&self, context: &GitContext, a: &str, e: &str, c: &str, o: &str) -> Result<String> {
        SimGit.advance(context, a, e, c, o)
    }
}
#[test]
fn hardening_tdd_freezes_red_baseline_separately_from_green_candidate() {
    let dir = tempfile::tempdir().unwrap();
    let mut e = engine(dir.path());
    e.git = Arc::new(DistinctCodeRevisions);
    let f = new_feature(&e);
    planning(&e, &f);
    let t = task(&e, &f, "code revisions");
    let (_, d) = implementation(&e, &t, "success");
    review(&e, &d);
    let s = e.store.snapshot().unwrap();
    let delivery = s.delivery(&d).unwrap();
    s.validate_evidence(delivery).unwrap();
    let red = s
        .runs
        .iter()
        .find(|r| delivery.evidence.contains(&r.id) && r.node == "red")
        .unwrap();
    assert_ne!(
        red.result.as_ref().unwrap()["candidate"],
        json!(delivery.candidate)
    );
}

// Completing H3: calls are gated by channels, never timing animations.
struct GatedHarness {
    entered: std::sync::mpsc::Sender<Run>,
    resume: std::sync::Mutex<std::sync::mpsc::Receiver<()>>,
    once: std::sync::atomic::AtomicBool,
}
impl HarnessPort for GatedHarness {
    fn configure(&self, n: &str, s: &str) -> String {
        SimHarness.configure(n, s)
    }
    fn poll(&self, r: &Run, input: &Value, now: u64) -> Result<Option<Value>> {
        if !self.once.swap(true, std::sync::atomic::Ordering::SeqCst) {
            self.entered.send(r.clone()).unwrap();
            self.resume
                .lock()
                .unwrap()
                .recv_timeout(std::time::Duration::from_secs(5))
                .map_err(|_| err("timeout", "writer blocked cancellation"))?;
        }
        Immediate.poll(r, input, now)
    }
}
fn gated_edit_invalidates_receipt(change_input: bool) {
    let dir = tempfile::tempdir().unwrap();
    let mut e = engine(dir.path());
    let f = new_feature(&e);
    call(&e, "attempt.start", &f, json!({"phase":"planning"}));
    e.tick(4).unwrap();
    if change_input {
        e.tick(4).unwrap();
    } // Gate the requirement output, not research.
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let (resume_tx, resume_rx) = std::sync::mpsc::channel();
    e.harness = Arc::new(GatedHarness {
        entered: entered_tx,
        resume: std::sync::Mutex::new(resume_rx),
        once: std::sync::atomic::AtomicBool::new(false),
    });
    let e = Arc::new(e);
    let worker_engine = e.clone();
    let worker = std::thread::spawn(move || worker_engine.tick(4));
    let run = entered_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap();
    let before = e.store.snapshot().unwrap();
    assert!(
        before
            .effects
            .iter()
            .any(|effect| effect.run_id == run.id && effect.status == EffectStatus::Pending)
    );
    if change_input {
        call(
            &e,
            "document.revise",
            &f,
            json!({"kind":"requirement","body":"用户修订后的唯一事实"}),
        );
    } else {
        call(&e, "run.cancel", &run.id, json!({}));
    }
    resume_tx.send(()).unwrap();
    worker.join().unwrap().unwrap();
    let s = e.store.snapshot().unwrap();
    let current = s.runs.iter().find(|r| r.id == run.id).unwrap();
    assert_eq!(current.status, Status::Running);
    assert!(current.result.is_none());
    assert!(current.cancel_requested);
    assert!(s.effects.iter().any(|effect| effect.run_id == run.id
        && effect.status == EffectStatus::Discarded
        && effect.receipt.is_some()));
    if change_input {
        assert_eq!(
            s.documents
                .iter()
                .filter(|d| d.kind == "requirement")
                .map(|d| d.body.as_str())
                .collect::<Vec<_>>(),
            vec!["用户修订后的唯一事实"]
        );
    }
    e.tick(4).unwrap();
    let s = e.store.snapshot().unwrap();
    assert_eq!(
        s.runs.iter().find(|r| r.id == run.id).unwrap().status,
        Status::Cancelled
    );
    assert!(
        s.resources
            .iter()
            .filter(|r| r.run_id == run.id)
            .all(|r| r.status == Status::Released)
    );
}
#[test]
fn completion_cancel_during_poll_discards_late_success() {
    gated_edit_invalidates_receipt(false);
}
#[test]
fn completion_input_change_during_poll_discards_late_document() {
    gated_edit_invalidates_receipt(true);
}

struct UnknownUntilConfirmed(Arc<std::sync::atomic::AtomicBool>);
impl HarnessPort for UnknownUntilConfirmed {
    fn configure(&self, _: &str, _: &str) -> String {
        "hold".into()
    }
    fn poll(&self, _: &Run, _: &Value, _: u64) -> Result<Option<Value>> {
        Ok(None)
    }
    fn reconcile(&self, r: &Run) -> Result<Option<Value>> {
        Ok(if self.0.load(std::sync::atomic::Ordering::SeqCst) {
            Some(
                json!({"status":"cancelled","run_id":r.id,"session_id":r.session_id,"cancellation_confirmed":true}),
            )
        } else {
            None
        })
    }
}
#[test]
fn completion_restart_keeps_unknown_resources_until_explicit_reconciliation() {
    let dir = tempfile::tempdir().unwrap();
    let known = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let rid;
    let fid;
    {
        let mut e = engine(dir.path());
        e.harness = Arc::new(UnknownUntilConfirmed(known.clone()));
        fid = new_feature(&e);
        call(&e, "attempt.start", &fid, json!({"phase":"planning"}));
        e.tick(4).unwrap();
        e.tick(4).unwrap();
        rid = e
            .store
            .snapshot()
            .unwrap()
            .runs
            .iter()
            .find(|r| r.status == Status::Running)
            .unwrap()
            .id
            .clone();
    }
    let e = engine_with_harness(dir.path(), Arc::new(UnknownUntilConfirmed(known.clone())));
    let snapshot = e.store.snapshot().unwrap();
    assert_eq!(
        snapshot.runs.iter().find(|r| r.id == rid).unwrap().status,
        Status::Interrupted
    );
    assert!(
        snapshot
            .resources
            .iter()
            .any(|resource| resource.run_id == rid && resource.status == Status::Active)
    );
    assert!(cmd(&e, "run.retry", Some(&rid), json!({})).is_err());
    let effect = snapshot
        .effects
        .iter()
        .find(|effect| effect.run_id == rid && effect.status == EffectStatus::NeedsReconciliation)
        .unwrap()
        .id
        .clone();
    known.store(true, std::sync::atomic::Ordering::SeqCst);
    call(&e, "effect.reconcile", &effect, json!({}));
    e.tick(4).unwrap();
    let s = e.store.snapshot().unwrap();
    assert_eq!(
        s.runs.iter().find(|r| r.id == rid).unwrap().status,
        Status::Interrupted
    );
    assert!(
        s.resources
            .iter()
            .filter(|resource| resource.run_id == rid)
            .all(|resource| resource.status == Status::Released)
    );
    assert!(cmd(&e, "run.retry", Some(&rid), json!({})).is_ok());
    assert!(
        e.query("query.workspace", &json!({"feature_id":fid}), 4)
            .unwrap()["detail"]["effects"]
            .is_array()
    );
}

struct CountPolls(Arc<std::sync::atomic::AtomicUsize>);
impl HarnessPort for CountPolls {
    fn configure(&self, n: &str, s: &str) -> String {
        Immediate.configure(n, s)
    }
    fn poll(&self, r: &Run, input: &Value, now: u64) -> Result<Option<Value>> {
        self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Immediate.poll(r, input, now)
    }
}
#[test]
fn completion_export_retry_reuses_persisted_receipt() {
    let dir = tempfile::tempdir().unwrap();
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let mut e = engine_with_harness(dir.path(), Arc::new(CountPolls(calls.clone())));
    let f = new_feature(&e);
    call(&e, "attempt.start", &f, json!({"phase":"planning"}));
    e.tick(4).unwrap();
    e.artifacts = Arc::new(FailingRunExport(FileArtifacts::new(
        dir.path().to_path_buf(),
    )));
    assert!(e.tick(4).is_err());
    assert!(e.tick(4).is_err());
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert!(
        e.store
            .snapshot()
            .unwrap()
            .effects
            .iter()
            .any(|effect| effect.status == EffectStatus::Observed && effect.receipt.is_some())
    );
    e.artifacts = Arc::new(FileArtifacts::new(dir.path().to_path_buf()));
    e.tick(4).unwrap();
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert!(
        e.store
            .snapshot()
            .unwrap()
            .effects
            .iter()
            .any(|effect| effect.kind == EffectKind::Cleanup
                && effect.status == EffectStatus::Applied)
    );
}
#[test]
fn completion_cleanup_retry_preserves_failed_history_and_is_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    let f = new_feature(&e);
    call(
        &e,
        "attempt.start",
        &f,
        json!({"phase":"planning","scenario":"cleanup_failure"}),
    );
    e.tick(4).unwrap();
    e.tick(4).unwrap();
    let old = e
        .store
        .snapshot()
        .unwrap()
        .effects
        .iter()
        .find(|effect| effect.kind == EffectKind::Cleanup && effect.status == EffectStatus::Failed)
        .unwrap()
        .clone();
    let c = Command {
        command_id: id(),
        target: Some(old.id.clone()),
        expected_revision: Some(old.revision),
        payload: json!({}),
    };
    let first = e.execute("effect.retry_cleanup", c.clone()).unwrap();
    assert_eq!(first, e.execute("effect.retry_cleanup", c.clone()).unwrap());
    e.tick(4).unwrap();
    let s = e.store.snapshot().unwrap();
    assert_eq!(
        s.effects
            .iter()
            .find(|effect| effect.id == old.id)
            .unwrap()
            .status,
        EffectStatus::Failed
    );
    assert_eq!(
        s.effects
            .iter()
            .find(|effect| effect.id == first["id"])
            .unwrap()
            .status,
        EffectStatus::Applied
    );
}
#[test]
fn completion_stage_outputs_do_not_replace_frozen_inputs() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    let f = new_feature(&e);
    let aid = planning(&e, &f);
    let s = e.store.snapshot().unwrap();
    let a = s.attempt(&aid).unwrap();
    assert!(a.documents.is_empty());
    assert_eq!(a.outputs.len(), 3);
    let design = s
        .documents
        .iter()
        .find(|d| a.outputs.contains(&d.id) && d.kind == "design")
        .unwrap();
    let test = s
        .documents
        .iter()
        .find(|d| a.outputs.contains(&d.id) && d.kind == "test")
        .unwrap();
    assert_eq!(test.design_id.as_ref(), Some(&design.id));
    let test_run = s
        .runs
        .iter()
        .find(|r| r.attempt_id.as_deref() == Some(&aid) && r.node == "test_plan")
        .unwrap();
    let request = &s
        .effects
        .iter()
        .find(|effect| effect.run_id == test_run.id && effect.kind == EffectKind::Harness)
        .unwrap()
        .request;
    assert!(
        request["input"]["frozen_inputs"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        request["input"]["stage_outputs"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["id"] == design.id)
    );
}
#[test]
fn completion_final_snapshot_captures_executed_context() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    let f = new_feature(&e);
    planning(&e, &f);
    let t = task(&e, &f, "context");
    let (a, d) = implementation(&e, &t, "success");
    let frozen = e
        .store
        .snapshot()
        .unwrap()
        .attempt(&a)
        .unwrap()
        .context_content
        .clone();
    let context = e
        .store
        .snapshot()
        .unwrap()
        .contexts
        .iter()
        .find(|c| c.owner_id == t)
        .unwrap()
        .id
        .clone();
    call(
        &e,
        "context.update",
        &context,
        json!({"content":"未参与该交付的新上下文"}),
    );
    review(&e, &d);
    assert_eq!(
        e.store
            .snapshot()
            .unwrap()
            .snapshots
            .iter()
            .find(|s| s.delivery_id.as_deref() == Some(&d))
            .unwrap()
            .content,
        frozen
    );
}
struct AmbiguousGit {
    calls: std::sync::atomic::AtomicUsize,
    known: std::sync::atomic::AtomicBool,
}
impl GitPort for AmbiguousGit {
    fn execution_revision(&self, a: &Attempt, p: &str) -> Result<String> {
        SimGit.execution_revision(a, p)
    }
    fn candidate(&self, context: &GitContext, s: &str, t: &str, o: &str) -> Result<String> {
        SimGit.candidate(context, s, t, o)
    }
    fn advance(&self, _context: &GitContext, _: &str, _: &str, _: &str, _: &str) -> Result<String> {
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Err(err("timeout", "外部已接收请求，回执丢失"))
    }
    fn reconcile_advance(&self, _context: &GitContext, id: &str) -> Result<AdvanceObservation> {
        Ok(if self.known.load(std::sync::atomic::Ordering::SeqCst) {
            AdvanceObservation::Applied {
                revision: format!("sim-rev-candidate-{id}"),
            }
        } else {
            AdvanceObservation::Unknown
        })
    }
}
#[test]
fn completion_git_unknown_outcome_is_reconciled_without_duplicate_advance() {
    let dir = tempfile::tempdir().unwrap();
    let provider = Arc::new(AmbiguousGit {
        calls: std::sync::atomic::AtomicUsize::new(0),
        known: std::sync::atomic::AtomicBool::new(false),
    });
    let mut e = engine(dir.path());
    e.git = provider.clone();
    let f = new_feature(&e);
    planning(&e, &f);
    let t = task(&e, &f, "merge");
    let (_, d) = implementation(&e, &t, "success");
    review(&e, &d);
    let m = merge(&e, &t);
    let c = Command {
        command_id: id(),
        target: Some(m.clone()),
        expected_revision: revision(&e, &m),
        payload: json!({}),
    };
    let first = e.execute("merge.apply", c.clone()).unwrap();
    assert_eq!(first["operation_id"], m);
    assert_eq!(provider.calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    e.tick(4).unwrap();
    assert_eq!(
        e.store.snapshot().unwrap().merge(&m).unwrap().status,
        Status::Accepted
    );
    assert_eq!(
        e.store.snapshot().unwrap().advances[0].status,
        AdvanceStatus::NeedsReconciliation
    );
    assert_eq!(first, e.execute("merge.apply", c.clone()).unwrap());
    assert_eq!(provider.calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    drop(e);
    let mut e = engine(dir.path());
    e.git = provider.clone();
    provider
        .known
        .store(true, std::sync::atomic::Ordering::SeqCst);
    call(&e, "merge.reconcile", &m, json!({}));
    e.tick(4).unwrap();
    assert_eq!(
        e.store.snapshot().unwrap().merge(&m).unwrap().status,
        Status::Completed
    );
    assert_eq!(
        e.store.snapshot().unwrap().advances[0].status,
        AdvanceStatus::Applied
    );
    assert_eq!(provider.calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(first, e.execute("merge.apply", c).unwrap());
    assert_eq!(provider.calls.load(std::sync::atomic::Ordering::SeqCst), 1);
}
#[test]
fn completion_binding_checks_capabilities_instead_of_provider_literal() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    let f = new_feature(&e);
    let mut s = e.store.snapshot().unwrap();
    let mut binding = s.feature(&f).unwrap().binding.clone();
    s.harnesses[0].capabilities.retain(|cap| cap != "cancel");
    assert!(s.validate_binding(&binding).is_err());
    s.harnesses[0].capabilities.push("cancel".into());
    let mut v2 = s.harnesses[0].clone();
    v2.id = "simulation-compatible-v2".into();
    v2.version = "2".into();
    binding.harness = v2.id.clone();
    s.harnesses.push(v2);
    assert!(s.validate_binding(&binding).is_ok());
}

struct CrashAfterAdvance {
    external: std::sync::Mutex<std::collections::HashMap<String, String>>,
    once: std::sync::atomic::AtomicBool,
    writes: std::sync::atomic::AtomicUsize,
}
impl GitPort for CrashAfterAdvance {
    fn execution_revision(&self, a: &Attempt, p: &str) -> Result<String> {
        SimGit.execution_revision(a, p)
    }
    fn candidate(&self, context: &GitContext, s: &str, t: &str, o: &str) -> Result<String> {
        SimGit.candidate(context, s, t, o)
    }
    fn advance(&self, _context: &GitContext, _: &str, _: &str, c: &str, o: &str) -> Result<String> {
        {
            let mut external = self.external.lock().unwrap();
            if !external.contains_key(o) {
                external.insert(o.into(), c.into());
                self.writes
                    .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            }
        }
        if self.once.swap(false, std::sync::atomic::Ordering::SeqCst) {
            panic!("fixture: process lost after external advance");
        }
        Ok(c.into())
    }
    fn reconcile_advance(&self, _context: &GitContext, o: &str) -> Result<AdvanceObservation> {
        Ok(self
            .external
            .lock()
            .unwrap()
            .get(o)
            .cloned()
            .map(|revision| AdvanceObservation::Applied { revision })
            .unwrap_or(AdvanceObservation::Unknown))
    }
}
#[test]
fn completion_git_crash_after_side_effect_recovers_from_original_provider_receipt() {
    let dir = tempfile::tempdir().unwrap();
    let provider = Arc::new(CrashAfterAdvance {
        external: std::sync::Mutex::new(std::collections::HashMap::new()),
        once: std::sync::atomic::AtomicBool::new(true),
        writes: std::sync::atomic::AtomicUsize::new(0),
    });
    let mut e = engine(dir.path());
    e.git = provider.clone();
    let f = new_feature(&e);
    planning(&e, &f);
    let t = task(&e, &f, "crash");
    let (_, d) = implementation(&e, &t, "success");
    review(&e, &d);
    let m = merge(&e, &t);
    let c = Command {
        command_id: id(),
        target: Some(m.clone()),
        expected_revision: revision(&e, &m),
        payload: json!({}),
    };
    e.execute("merge.apply", c.clone()).unwrap();
    assert_eq!(provider.writes.load(std::sync::atomic::Ordering::SeqCst), 0);
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| e.tick(4))).is_err());
    assert_eq!(
        e.store.snapshot().unwrap().advances[0].status,
        AdvanceStatus::Dispatched
    );
    drop(e);
    let store = Arc::new(SqliteStore::open(dir.path()).unwrap());
    let simulated = devdesk_adapters::simulation::adapters();
    let e = Engine {
        store,
        artifacts: Arc::new(FileArtifacts::new(dir.path().to_path_buf())),
        harness: Arc::new(Immediate),
        git: provider.clone(),
        tests: simulated.tests,
        sandbox: Arc::new(SimSandbox),
        pull_requests: None,
        simulation: simulated.simulation,
    };
    e.bootstrap().unwrap();
    assert_eq!(
        e.store.snapshot().unwrap().merge(&m).unwrap().status,
        Status::Completed
    );
    assert_eq!(provider.writes.load(std::sync::atomic::Ordering::SeqCst), 1);
    e.execute("merge.apply", c).unwrap();
    assert_eq!(provider.writes.load(std::sync::atomic::Ordering::SeqCst), 1);
}
struct CrashAfterCleanup {
    attempts: std::sync::Mutex<Vec<String>>,
    deleted: std::sync::Mutex<std::collections::HashSet<String>>,
    once: std::sync::atomic::AtomicBool,
}
impl SandboxPort for CrashAfterCleanup {
    fn resource(&self, r: &str) -> Resource {
        SimSandbox.resource(r)
    }
    fn cleanup(&self, r: &mut Resource, _: bool) {
        self.attempts.lock().unwrap().push(r.id.clone());
        self.deleted.lock().unwrap().insert(r.id.clone());
        if self.once.swap(false, std::sync::atomic::Ordering::SeqCst) {
            panic!("fixture: process lost after external delete");
        }
        r.status = Status::Released;
    }
}
#[test]
fn completion_cleanup_crash_replays_original_resource_identity() {
    let dir = tempfile::tempdir().unwrap();
    let provider = Arc::new(CrashAfterCleanup {
        attempts: std::sync::Mutex::new(vec![]),
        deleted: std::sync::Mutex::new(std::collections::HashSet::new()),
        once: std::sync::atomic::AtomicBool::new(true),
    });
    let mut e = engine(dir.path());
    e.sandbox = provider.clone();
    let f = new_feature(&e);
    call(&e, "attempt.start", &f, json!({"phase":"planning"}));
    e.tick(4).unwrap();
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| e.tick(4))).is_err());
    let target = provider.attempts.lock().unwrap()[0].clone();
    assert!(
        e.store
            .snapshot()
            .unwrap()
            .effects
            .iter()
            .any(|effect| effect.kind == EffectKind::Cleanup
                && effect.status == EffectStatus::Pending)
    );
    drop(e);
    let store = Arc::new(SqliteStore::open(dir.path()).unwrap());
    let simulated = devdesk_adapters::simulation::adapters();
    let e = Engine {
        store,
        artifacts: Arc::new(FileArtifacts::new(dir.path().to_path_buf())),
        harness: Arc::new(Immediate),
        git: Arc::new(SimGit),
        tests: simulated.tests,
        sandbox: provider.clone(),
        pull_requests: None,
        simulation: simulated.simulation,
    };
    e.bootstrap().unwrap();
    assert_eq!(
        provider
            .attempts
            .lock()
            .unwrap()
            .iter()
            .filter(|id| *id == &target)
            .count(),
        2
    );
    assert!(provider.deleted.lock().unwrap().contains(&target));
    assert_eq!(
        e.store
            .snapshot()
            .unwrap()
            .resources
            .iter()
            .find(|r| r.id == target)
            .unwrap()
            .status,
        Status::Released
    );
}
#[test]
fn completion_feature_doc_edit_keeps_planning_and_sibling_merge_valid() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    let f = new_feature(&e);
    let p = planning(&e, &f);
    let t = task(&e, &f, "isolated");
    let (_, d) = implementation(&e, &t, "success");
    review(&e, &d);
    let m = merge(&e, &t);
    call(
        &e,
        "document.revise",
        &f,
        json!({"kind":"doc","body":"Feature 使用说明"}),
    );
    let s = e.store.snapshot().unwrap();
    assert!(s.planning_accepted(&f));
    assert!(
        s.deliveries
            .iter()
            .filter(|d| d.attempt_id.as_deref() == Some(&p))
            .all(|d| !d.stale)
    );
    assert_eq!(s.merge(&m).unwrap().status, Status::Accepted);
    call(&e, "merge.apply", &m, json!({}));
    assert_eq!(
        e.store.snapshot().unwrap().task(&t).unwrap().status,
        Status::Completed
    );
}

#[test]
fn completion_reviewer_and_merge_keep_source_standard_when_feature_defaults_change() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    let f = new_feature(&e);
    planning(&e, &f);
    let t = task(&e, &f, "locked");
    let (aid, did) = implementation(&e, &t, "success");
    let source = e
        .store
        .snapshot()
        .unwrap()
        .attempt(&aid)
        .unwrap()
        .binding
        .clone();
    let standard = result_id(call(
        &e,
        "definition.revise",
        &source.standard,
        json!({"body":"新默认标准"}),
    ));
    let mut next = source.clone();
    next.standard = standard;
    call(&e, "binding.update", &f, json!(next));
    let rid = result_id(call(&e, "review.request", &did, json!({})));
    assert_eq!(
        e.store
            .snapshot()
            .unwrap()
            .runs
            .iter()
            .find(|r| r.id == rid)
            .unwrap()
            .binding
            .standard,
        source.standard
    );
    tick(&e);
    let m = merge(&e, &t);
    let s = e.store.snapshot().unwrap();
    assert!(
        s.runs
            .iter()
            .filter(|r| r.merge_id.as_deref() == Some(&m))
            .all(|r| r.binding.standard == source.standard)
    );
    let request = &s
        .effects
        .iter()
        .find(|effect| effect.run_id == rid && effect.kind == EffectKind::Harness)
        .unwrap()
        .request;
    assert!(
        request["input"]["definitions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|definition| definition["id"] == source.standard)
    );
}
#[test]
fn completion_context_history_and_delivery_scope_are_enforced() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    let f = new_feature(&e);
    planning(&e, &f);
    let t = task(&e, &f, "scope");
    let (_, d) = implementation(&e, &t, "success");
    let context = e
        .store
        .snapshot()
        .unwrap()
        .contexts
        .iter()
        .find(|context| context.owner_id == t)
        .unwrap()
        .clone();
    call(
        &e,
        "context.update",
        &context.id,
        json!({"content":"新上下文修订"}),
    );
    let s = e.store.snapshot().unwrap();
    let event = s
        .events
        .iter()
        .find(|event| event.target == context.id && event.kind == "context.revised")
        .unwrap();
    let change: Value = serde_json::from_str(&event.detail).unwrap();
    assert_eq!(change["previous"], context.content);
    assert_eq!(change["content"], "新上下文修订");
    let mut wrong = s.delivery(&d).unwrap().clone();
    wrong.kind = "closure".into();
    assert!(s.validate_evidence(&wrong).is_err());
    let mut wrong = s.clone();
    wrong
        .documents
        .iter_mut()
        .find(|doc| doc.task_id.as_deref() == Some(&t) && doc.kind == "test")
        .unwrap()
        .task_id = Some("other-task".into());
    assert!(
        wrong
            .validate_evidence(wrong.delivery(&d).unwrap())
            .is_err()
    );
}

struct TargetChangedBeforeAdvance;
impl GitPort for TargetChangedBeforeAdvance {
    fn execution_revision(&self, a: &Attempt, p: &str) -> Result<String> {
        SimGit.execution_revision(a, p)
    }
    fn candidate(&self, context: &GitContext, s: &str, t: &str, o: &str) -> Result<String> {
        SimGit.candidate(context, s, t, o)
    }
    fn advance(&self, context: &GitContext, a: &str, e: &str, c: &str, o: &str) -> Result<String> {
        if e == "sim-rev-root" {
            Err(err("target_changed", "外部目标已变化，尚未推进"))
        } else {
            SimGit.advance(context, a, e, c, o)
        }
    }
    fn reconcile_advance(&self, _context: &GitContext, _: &str) -> Result<AdvanceObservation> {
        Ok(AdvanceObservation::NotApplied {
            actual_target: "sim-rev-external-target".into(),
        })
    }
}
#[test]
fn completion_confirmed_not_applied_refreshes_target_and_requires_new_candidate() {
    let dir = tempfile::tempdir().unwrap();
    let mut e = engine(dir.path());
    e.git = Arc::new(TargetChangedBeforeAdvance);
    let f = new_feature(&e);
    planning(&e, &f);
    let t = task(&e, &f, "external target");
    let (_, d) = implementation(&e, &t, "success");
    review(&e, &d);
    let m = merge(&e, &t);
    call(&e, "merge.apply", &m, json!({}));
    call(&e, "merge.reconcile", &m, json!({}));
    e.tick(4).unwrap();
    let s = e.store.snapshot().unwrap();
    assert_eq!(s.advances[0].status, AdvanceStatus::NotApplied);
    assert_eq!(s.merge(&m).unwrap().status, Status::Interrupted);
    assert!(
        s.deliveries
            .iter()
            .filter(|d| d.merge_id.as_deref() == Some(&m))
            .all(|d| d.stale)
    );
    assert!(s.accepted(s.delivery(&d).unwrap()));
    let next = merge(&e, &t);
    assert_ne!(next, m);
    assert_eq!(
        e.store
            .snapshot()
            .unwrap()
            .merge(&next)
            .unwrap()
            .expected_target,
        "sim-rev-external-target"
    );
    call(&e, "merge.apply", &next, json!({}));
    assert_eq!(
        e.store.snapshot().unwrap().task(&t).unwrap().status,
        Status::Completed
    );
}
struct WrongRecoveryIdentity;
impl HarnessPort for WrongRecoveryIdentity {
    fn configure(&self, _: &str, _: &str) -> String {
        "hold".into()
    }
    fn poll(&self, _: &Run, _: &Value, _: u64) -> Result<Option<Value>> {
        Ok(None)
    }
    fn reconcile(&self, r: &Run) -> Result<Option<Value>> {
        Ok(Some(
            json!({"status":"cancelled","run_id":"other-run","session_id":r.session_id,"cancellation_confirmed":true}),
        ))
    }
}
#[test]
fn completion_wrong_reconciliation_receipt_does_not_release_owned_resources() {
    let dir = tempfile::tempdir().unwrap();
    let rid;
    {
        let e = engine_with_harness(dir.path(), Arc::new(WrongRecoveryIdentity));
        let f = new_feature(&e);
        call(&e, "attempt.start", &f, json!({"phase":"planning"}));
        e.tick(4).unwrap();
        e.tick(4).unwrap();
        rid = e
            .store
            .snapshot()
            .unwrap()
            .runs
            .iter()
            .find(|r| r.status == Status::Running)
            .unwrap()
            .id
            .clone();
    }
    let e = engine_with_harness(dir.path(), Arc::new(WrongRecoveryIdentity));
    let s = e.store.snapshot().unwrap();
    assert!(
        s.effects.iter().any(
            |effect| effect.run_id == rid && effect.status == EffectStatus::NeedsReconciliation
        )
    );
    assert!(
        s.resources
            .iter()
            .any(|resource| resource.run_id == rid && resource.status == Status::Active)
    );
    assert!(cmd(&e, "run.retry", Some(&rid), json!({})).is_err());
}

#[test]
fn completion_git_and_test_requests_freeze_workspace_and_runtime_routing() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    let f = new_feature(&e);
    let initial = e.store.snapshot().unwrap();
    let project = initial.feature(&f).unwrap().project_id.clone();
    call(
        &e,
        "project.update",
        &project,
        json!({"workspace_definition":{"repository":"logical-repository-A","path":"/logical/A","mainline":"trunk"}}),
    );
    planning(&e, &f);
    let t = task(&e, &f, "routing");
    let (aid, d) = implementation(&e, &t, "success");
    review(&e, &d);
    call(
        &e,
        "project.update",
        &project,
        json!({"workspace_definition":{"repository":"logical-repository-B","path":"/logical/B","mainline":"other"}}),
    );
    let m = merge(&e, &t);
    e.execute(
        "merge.apply",
        Command {
            command_id: id(),
            target: Some(m.clone()),
            expected_revision: revision(&e, &m),
            payload: json!({}),
        },
    )
    .unwrap();
    let s = e.store.snapshot().unwrap();
    let context = s
        .advances
        .iter()
        .find(|a| a.id == m)
        .unwrap()
        .context
        .as_ref()
        .unwrap();
    assert_eq!(context.workspace.repository, "logical-repository-A");
    assert_eq!(context.workspace.path, "/logical/A");
    assert_eq!(context.source.owner, t);
    assert_eq!(context.target.owner, f);
    assert_eq!(context.source.branch, format!("task/{t}"));
    let green = s
        .runs
        .iter()
        .find(|r| r.attempt_id.as_deref() == Some(&aid) && r.node == "green")
        .unwrap();
    let request: &Value = &s
        .effects
        .iter()
        .find(|effect| effect.run_id == green.id && effect.kind == EffectKind::Test)
        .unwrap()
        .request["test"];
    assert_eq!(request["code"]["workspace"]["path"], "/logical/A");
    assert!(!request["definitions"].as_array().unwrap().is_empty());
    assert!(
        request["runtime"]["external_environment"]
            .as_str()
            .unwrap()
            .starts_with("sim-environment-")
    );
    e.tick(4).unwrap();
    assert_eq!(
        e.store.snapshot().unwrap().task(&t).unwrap().status,
        Status::Completed
    );
}
#[test]
fn completion_invalidated_advance_intent_never_calls_provider() {
    let dir = tempfile::tempdir().unwrap();
    let provider = Arc::new(AmbiguousGit {
        calls: std::sync::atomic::AtomicUsize::new(0),
        known: std::sync::atomic::AtomicBool::new(false),
    });
    let mut e = engine(dir.path());
    e.git = provider.clone();
    let f = new_feature(&e);
    planning(&e, &f);
    let t = task(&e, &f, "invalidation");
    let (_, d) = implementation(&e, &t, "success");
    review(&e, &d);
    let m = merge(&e, &t);
    e.execute(
        "merge.apply",
        Command {
            command_id: id(),
            target: Some(m.clone()),
            expected_revision: revision(&e, &m),
            payload: json!({}),
        },
    )
    .unwrap();
    call(
        &e,
        "document.revise",
        &f,
        json!({"kind":"requirement","body":"推进前需求变化"}),
    );
    e.tick(4).unwrap();
    assert_eq!(provider.calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    assert_eq!(
        e.store.snapshot().unwrap().advances[0].status,
        AdvanceStatus::NotApplied
    );
    assert_ne!(
        e.store.snapshot().unwrap().merge(&m).unwrap().status,
        Status::Completed
    );
}

struct GreenWaitsForSibling(std::sync::atomic::AtomicUsize);
impl HarnessPort for GreenWaitsForSibling {
    fn configure(&self, n: &str, s: &str) -> String {
        SimHarness.configure(n, s)
    }
    fn poll(&self, r: &Run, input: &Value, now: u64) -> Result<Option<Value>> {
        if r.node == "green" && self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
            return Ok(None);
        }
        Immediate.poll(r, input, now)
    }
}
#[test]
fn completion_parallel_output_does_not_change_existing_run_request() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine_with_harness(
        dir.path(),
        Arc::new(GreenWaitsForSibling(std::sync::atomic::AtomicUsize::new(0))),
    );
    let f = new_feature(&e);
    planning(&e, &f);
    let t = task(&e, &f, "parallel");
    let (aid, d) = implementation(&e, &t, "success");
    review(&e, &d);
    let s = e.store.snapshot().unwrap();
    assert_eq!(s.attempt(&aid).unwrap().status, Status::Accepted);
    let green = s
        .runs
        .iter()
        .find(|r| r.attempt_id.as_deref() == Some(&aid) && r.node == "green")
        .unwrap();
    let request = &s
        .effects
        .iter()
        .find(|effect| effect.run_id == green.id && effect.kind == EffectKind::Harness)
        .unwrap()
        .request;
    assert!(
        request["input"]["stage_outputs"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(!s.attempt(&aid).unwrap().outputs.is_empty());
}
#[test]
fn completion_agent_revision_is_not_a_separate_reviewer_identity() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    let f = new_feature(&e);
    planning(&e, &f);
    let t = task(&e, &f, "agent revision");
    let mut binding = e
        .store
        .snapshot()
        .unwrap()
        .task(&t)
        .unwrap()
        .binding
        .clone();
    let original = binding.agent.clone();
    binding.agent = result_id(call(
        &e,
        "definition.revise",
        &original,
        json!({"body":"工程 Agent v2"}),
    ));
    call(&e, "binding.update", &t, json!(binding));
    let (_, d) = implementation(&e, &t, "success");
    let review = result_id(call(&e, "review.request", &d, json!({})));
    let s = e.store.snapshot().unwrap();
    let reviewer = &s
        .runs
        .iter()
        .find(|r| r.id == review)
        .unwrap()
        .binding
        .agent;
    assert_ne!(
        s.agent_identity(reviewer).unwrap(),
        s.agent_identity(&original).unwrap()
    );
    tick(&e);
    assert!(
        e.store
            .snapshot()
            .unwrap()
            .accepted(e.store.snapshot().unwrap().delivery(&d).unwrap())
    );
}

#[test]
fn project_databases_migrate_full_history_and_keep_the_legacy_source() {
    let dir = tempfile::tempdir().unwrap();
    let old = engine(dir.path());
    let f1 = new_feature(&old);
    planning(&old, &f1);
    let tid = task(&old, &f1, "独立项目任务");
    let (_, delivery) = implementation(&old, &tid, "success");
    review(&old, &delivery);
    let mid = merge(&old, &tid);
    call(&old, "merge.apply", &mid, json!({}));
    let f2 = new_feature(&old);
    planning(&old, &f2);
    let spec = old
        .store
        .snapshot()
        .unwrap()
        .definitions
        .iter()
        .find(|d| d.family == "spec")
        .unwrap()
        .id
        .clone();
    call(
        &old,
        "definition.revise",
        &spec,
        json!({"body":"共享规范的新修订"}),
    );
    let before = serde_json::to_value(old.store.snapshot().unwrap()).unwrap();
    std::fs::write(
        dir.path().join("runtime.json"),
        r#"{"global_concurrency":7}"#,
    )
    .unwrap();
    assert!(GlobalStore::open(dir.path()).is_err());
    drop(old);
    let original = std::fs::read(dir.path().join("engineering.sqlite3")).unwrap();
    let global = GlobalStore::open(dir.path()).unwrap();
    assert_eq!(global.concurrency().unwrap(), 7);
    let (projects, count) = global.projects(0, 100).unwrap();
    assert_eq!(count, 2);
    let mut combined = std::collections::HashMap::<String, usize>::new();
    let global_state = global.snapshot().unwrap();
    let mut events = global_state.events.len();
    for project in &projects {
        let store = SqliteStore::open_project(dir.path(), &project.id).unwrap();
        let state = store.snapshot().unwrap();
        assert_eq!(state.projects.len(), 1);
        assert_eq!(state.projects[0].id, project.id);
        assert_eq!(state.features.len(), 1);
        assert!(state.features.iter().all(|f| f.project_id == project.id));
        let artifacts = FileArtifacts::new(store.root().into());
        for artifact in &state.artifacts {
            assert!(!artifacts.read(artifact).unwrap().is_empty());
        }
        events += state.events.len();
        let json = serde_json::to_value(&state).unwrap();
        for (table, rows) in json.as_object().unwrap() {
            *combined.entry(table.clone()).or_default() += rows.as_array().unwrap().len();
        }
        let conn = rusqlite::Connection::open(
            std::path::PathBuf::from(store.root()).join("engineering.sqlite3"),
        )
        .unwrap();
        assert_eq!(
            conn.query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| {
                r.get::<_, usize>(0)
            })
            .unwrap(),
            0
        );
        let fp:String=conn.query_row("SELECT fingerprint FROM operations WHERE json_extract(fingerprint,'$[0]')='project.create'",[],|r|r.get(0)).unwrap();
        let command_id: String = conn
            .query_row(
                "SELECT id FROM operations WHERE fingerprint=?1",
                [&fp],
                |r| r.get(0),
            )
            .unwrap();
        let values: Value = serde_json::from_str(&fp).unwrap();
        let command = Command {
            command_id,
            target: None,
            expected_revision: None,
            payload: values[3].clone(),
        };
        assert_eq!(global.reserve_project(&command).unwrap(), project.id);
    }
    for (table, rows) in before.as_object().unwrap() {
        if ![
            "permissions",
            "definitions",
            "harnesses",
            "sandboxes",
            "events",
        ]
        .contains(&table.as_str())
        {
            assert_eq!(
                combined[table],
                rows.as_array().unwrap().len(),
                "lost {table}"
            );
        }
    }
    assert_eq!(events, before["events"].as_array().unwrap().len());
    assert_eq!(
        serde_json::to_value(global_state.definitions).unwrap(),
        before["definitions"]
    );
    let conn = rusqlite::Connection::open(dir.path().join("global.sqlite3")).unwrap();
    assert_eq!(conn.query_row("SELECT count(*) FROM sqlite_master WHERE type='table' AND name IN ('features','runs','projects','deliveries')",[],|r|r.get::<_,usize>(0)).unwrap(),0);
    assert_eq!(
        original,
        std::fs::read(dir.path().join("engineering.sqlite3")).unwrap()
    );
    drop(conn);
    drop(global);
    let reopened = GlobalStore::open(dir.path()).unwrap();
    assert_eq!(reopened.projects(0, 100).unwrap().1, 2);
    assert_eq!(reopened.concurrency().unwrap(), 7);
}

#[test]
fn project_database_migration_retries_after_artifact_failure_without_publishing_partial_state() {
    let dir = tempfile::tempdir().unwrap();
    let old = engine(dir.path());
    let f1 = new_feature(&old);
    planning(&old, &f1);
    let f2 = new_feature(&old);
    planning(&old, &f2);
    let artifact = old
        .store
        .snapshot()
        .unwrap()
        .artifacts
        .iter()
        .find(|a| a.feature_id.as_deref() == Some(f2.as_str()))
        .unwrap()
        .clone();
    let path = dir.path().join(&artifact.relative_path);
    let bytes = std::fs::read(&path).unwrap();
    drop(old);
    std::fs::write(&path, b"corrupt").unwrap();
    assert!(GlobalStore::open(dir.path()).is_err());
    let conn = rusqlite::Connection::open(dir.path().join("global.sqlite3")).unwrap();
    assert_eq!(
        conn.query_row("SELECT count(*) FROM project_catalog", [], |r| r
            .get::<_, usize>(0))
            .unwrap(),
        0
    );
    drop(conn);
    std::fs::write(path, bytes).unwrap();
    let global = GlobalStore::open(dir.path()).unwrap();
    assert_eq!(global.projects(0, 100).unwrap().1, 2);
}

#[test]
fn global_configuration_upgrades_a_copy_of_v1_and_rejects_project_data() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join(".devdesk-dataset"), "devdesk-schema-v1\n").unwrap();
    let conn = rusqlite::Connection::open(dir.path().join("engineering.sqlite3")).unwrap();
    conn.execute_batch(include_str!("../../../sql/001_initial.sql"))
        .unwrap();
    drop(conn);
    let global = GlobalStore::open(dir.path()).unwrap();
    assert!(!global.snapshot().unwrap().definitions.is_empty());
    let mut state = global.snapshot().unwrap();
    let other = tempfile::tempdir().unwrap();
    let old = engine(&other.path().join("legacy"));
    new_feature(&old);
    state.projects = old.store.snapshot().unwrap().projects;
    assert!(
        global
            .transact(None, &mut |s| {
                s.projects = state.projects.clone();
                Ok(json!({}))
            })
            .is_err()
    );
    let conn = rusqlite::Connection::open(dir.path().join("engineering.sqlite3")).unwrap();
    assert_eq!(
        conn.query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))
            .unwrap(),
        1
    );
    global.set_concurrency(9).unwrap();
    assert_eq!(global.concurrency().unwrap(), 9);
    assert!(global.set_concurrency(0).is_err());
    assert!(global.set_concurrency(33).is_err());
}

#[cfg(unix)]
#[test]
fn global_and_project_database_paths_reject_symlink_redirection() {
    use std::os::unix::fs::symlink;
    let dir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join(".devdesk-dataset"), "devdesk-schema-v1\n").unwrap();
    std::fs::write(outside.path().join("db"), b"do not modify").unwrap();
    symlink(outside.path().join("db"), dir.path().join("global.sqlite3")).unwrap();
    assert!(GlobalStore::open(dir.path()).is_err());
    assert_eq!(
        std::fs::read(outside.path().join("db")).unwrap(),
        b"do not modify"
    );
    std::fs::remove_file(dir.path().join("global.sqlite3")).unwrap();
    symlink(outside.path(), dir.path().join("projects")).unwrap();
    assert!(GlobalStore::open(dir.path()).is_err());
    assert!(SqliteStore::open_project(dir.path(), &id()).is_err());
    std::fs::remove_file(dir.path().join("projects")).unwrap();
    std::fs::create_dir(dir.path().join("projects")).unwrap();
    let pid = id();
    symlink(outside.path(), dir.path().join("projects").join(&pid)).unwrap();
    assert!(SqliteStore::open_project(dir.path(), &pid).is_err());
    assert!(SqliteStore::open_project(dir.path(), "../escape").is_err());
}

/// An independent provider implementing the four public base contracts.
/// Simulation is used only for deterministic fixture responses.
#[derive(Default)]
struct RecordingProviders(std::sync::Mutex<std::collections::HashSet<&'static str>>);
impl RecordingProviders {
    fn record(&self, method: &'static str) {
        self.0.lock().unwrap().insert(method);
    }
}
impl devdesk_core::ports::Harness for RecordingProviders {
    fn identity(&self) -> String {
        "fixture:harness:1".into()
    }
    fn configure(&self, node: &str, scenario: &str) -> String {
        self.record("harness.configure");
        SimHarness.configure(node, scenario)
    }
    fn poll(&self, run: &Run, input: &Value, now: u64) -> Result<Option<Value>> {
        self.record("harness.poll");
        SimHarness.poll(run, input, now + 10_000)
    }
}
impl devdesk_core::ports::Git for RecordingProviders {
    fn identity(&self) -> String {
        "fixture:git:1".into()
    }
    fn execution_revision(&self, attempt: &Attempt, phase: &str) -> Result<String> {
        self.record("git.execution_revision");
        SimGit.execution_revision(attempt, phase)
    }
    fn candidate(
        &self,
        context: &GitContext,
        source: &str,
        target: &str,
        operation: &str,
    ) -> Result<String> {
        self.record("git.candidate");
        SimGit.candidate(context, source, target, operation)
    }
    fn advance(
        &self,
        context: &GitContext,
        actual: &str,
        expected: &str,
        candidate: &str,
        operation: &str,
    ) -> Result<String> {
        self.record("git.advance");
        SimGit.advance(context, actual, expected, candidate, operation)
    }
}
impl devdesk_core::ports::Tests for RecordingProviders {
    fn identity(&self) -> String {
        "fixture:tests:1".into()
    }
    fn validate(&self, request: &TestRequest) -> Value {
        self.record("tests.validate");
        SimTests::default().validate(request)
    }
}
impl devdesk_core::ports::Sandbox for RecordingProviders {
    fn identity(&self) -> String {
        "fixture:sandbox:1".into()
    }
    fn resource(&self, run: &str) -> Resource {
        self.record("sandbox.resource");
        SimSandbox.resource(run)
    }
    fn cleanup(&self, resource: &mut Resource, fail: bool) {
        self.record("sandbox.cleanup");
        SimSandbox.cleanup(resource, fail)
    }
}
#[test]
fn execution_base_contracts_allow_independent_providers_through_a_complete_task_merge() {
    let dir = tempfile::tempdir().unwrap();
    let initial = engine(dir.path());
    let provider = Arc::new(RecordingProviders::default());
    let e = Engine::new(
        initial.store,
        initial.artifacts,
        devdesk_core::ports::ExecutionAdapters {
            harness: provider.clone(),
            git: provider.clone(),
            tests: provider.clone(),
            sandbox: provider.clone(),
            pull_requests: None,
            simulation: None,
        },
    );
    let fid = new_feature(&e);
    planning(&e, &fid);
    let tid = task(&e, &fid, "独立实现与模拟实现共用契约");
    let (_, did) = implementation(&e, &tid, "success");
    review(&e, &did);
    let mid = merge(&e, &tid);
    call(&e, "merge.apply", &mid, json!({}));
    let s = e.store.snapshot().unwrap();
    assert_eq!(s.task(&tid).unwrap().status, Status::Completed);
    assert!(s.resources.iter().all(|r| r.status == Status::Released));
    let called = provider.0.lock().unwrap();
    for method in [
        "harness.configure",
        "harness.poll",
        "git.execution_revision",
        "git.candidate",
        "git.advance",
        "tests.validate",
        "sandbox.resource",
        "sandbox.cleanup",
    ] {
        assert!(
            called.contains(method),
            "custom contract was bypassed: {method}"
        );
    }
    assert!(s.effects.iter().all(|e| {
        e.request["adapter"]
            .as_str()
            .unwrap()
            .starts_with("fixture:")
    }));
    assert!(s.advances.iter().all(|a| a.provider == "fixture:git:1"));
}
#[test]
fn simulation_provider_identities_match_existing_persisted_history() {
    let adapters = devdesk_adapters::simulation::adapters();
    assert_eq!(adapters.harness.identity(), "devdesk_adapters::SimHarness");
    assert_eq!(adapters.git.identity(), "devdesk_adapters::SimGit");
    assert_eq!(adapters.tests.identity(), "devdesk_adapters::SimTests");
    assert_eq!(adapters.sandbox.identity(), "devdesk_adapters::SimSandbox");
}
