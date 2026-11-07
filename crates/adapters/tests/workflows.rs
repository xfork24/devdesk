use devdesk_adapters::*;
use devdesk_core::*;
use devdesk_domain::*;
use serde_json::{Value, json};
use std::{path::Path, sync::Arc};
struct Immediate;
impl HarnessPort for Immediate {
    fn configure(&self, node: &str, scenario: &str) -> String {
        SimHarness.configure(node, scenario)
    }
    fn poll(&self, r: &Run, input: &Value, now: u64) -> Result<Option<Value>> {
        SimHarness.poll(r, input, now + 10_000)
    }
}
fn engine(path: &Path) -> Engine {
    let store = Arc::new(SqliteStore::open(path).unwrap());
    let artifacts = Arc::new(FileArtifacts::new(path.to_path_buf()));
    let e = Engine {
        store,
        artifacts,
        harness: Arc::new(Immediate),
        git: Arc::new(SimGit),
        tests: Arc::new(SimTests),
        sandbox: Arc::new(SimSandbox),
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
    cmd(e, m, if t.is_empty() { None } else { Some(t) }, p)
        .unwrap_or_else(|err| panic!("{m} {t}: {err}"))
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
                        id: sid.clone(),
                        feature_id: fid.clone(),
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
