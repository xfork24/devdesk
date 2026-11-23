use super::*;

#[test]
fn delegated_completion_must_fit_the_finite_operation_budget() {
    let directory = tempfile::tempdir().unwrap();
    let e = engine(directory.path());
    let mut doc = document();
    let steps: Vec<_> = (0..32).map(|n| json!({"key":format!("step{n}"),"name":format!("步骤{n}"),"stage":"work","executor":{"type":"control"}})).collect();
    doc["steps"] = json!(steps);
    let root = start(&e, doc);
    let keys: Vec<_> = (0..32).map(|n| format!("step{n}")).collect();
    let error = cmd(
        &e,
        "workflow.segment.dispatch",
        Some(&root),
        json!({"steps":keys,"complete_instance":true,"reason":"32 次派发和接纳后已无收尾预算"}),
    )
    .unwrap_err();
    assert!(error.message.contains("操作预算"));
    // The existing 64-operation dispatch/accept-only grant remains valid.
    cmd(
        &e,
        "workflow.segment.dispatch",
        Some(&root),
        json!({"steps":keys,"reason":"保留旧委托语义"}),
    )
    .unwrap();
}

#[test]
fn overlapping_delegations_share_one_exact_human_request() {
    let directory = tempfile::tempdir().unwrap();
    let e = engine(directory.path());
    let mut doc = document();
    doc["steps"][0]["confirmation"] = json!("human");
    let root = start(&e, doc);
    for _ in 0..2 {
        cmd(&e,"workflow.segment.dispatch",Some(&root),json!({"steps":["one","two"],"include_human":true,
            "prepare_human_requests":true,"complete_instance":true,"reason":"重复授权仍只准备一个确切签核"})).unwrap();
    }
    for _ in 0..16 {
        e.tick(1).unwrap();
    }
    let state = e.store.snapshot().unwrap();
    assert_eq!(
        state
            .governance
            .iter()
            .filter(|r| r.r#type == "workflow_pending_decision" && r.definition_id == root)
            .count(),
        1
    );
    assert!(
        !state
            .governance
            .iter()
            .any(|r| r.r#type == "workflow_confirmation")
    );
    assert_ne!(runtime(&e, &root)["runtime"]["status"], "completed");
}

#[test]
fn delegated_completion_keeps_the_whole_workflow_rule() {
    let directory = tempfile::tempdir().unwrap();
    let e = engine(directory.path());
    let mut doc = document();
    doc["completion"] = json!({"op":"equals","source":{"step":"$input","port":"assessment","pointer":"/approve"},"value":true});
    doc["inputs"] =
        json!({"assessment":{"delivery_ref":"fixture-delivery-object:1","required":false}});
    let root = start(&e, doc);
    let grant = cmd(
        &e,
        "workflow.segment.dispatch",
        Some(&root),
        json!({"steps":["one","two"],"complete_instance":true,"reason":"步骤完成不能替代整体规则"}),
    )
    .unwrap();
    for _ in 0..12 {
        e.tick(1).unwrap();
    }
    let view = runtime(&e, &root);
    assert!(
        view["runtime"]["steps"]
            .as_array()
            .unwrap()
            .iter()
            .all(|s| s["status"] == "completed")
    );
    assert_ne!(view["runtime"]["status"], "completed");
    assert_eq!(
        e.store
            .snapshot()
            .unwrap()
            .configuration(grant["id"].as_str().unwrap())
            .unwrap()
            .data["status"],
        "stopped"
    );
}

#[test]
fn automated_delegation_does_not_wake_manager_for_ordinary_approval_or_completion() {
    let directory = tempfile::tempdir().unwrap();
    let mut e = engine(directory.path());
    let harness = Arc::new(NativeManager {
        segment: true,
        segment_wait: true,
        automated_segment: true,
        ..Default::default()
    });
    e.harness = harness;
    let mut doc = document();
    doc["steps"][0]["confirmation"] = json!("human");
    let root = start(&e, doc);
    let controller=cmd(&e,"orchestration.open",Some(&root),json!({"role_ref":"builtin-role-project-manager:1","agent_ref":"builtin-agent-gpt-6-2-sol-project-manager:4"})).unwrap()["id"].as_str().unwrap().to_owned();
    cmd(
        &e,
        "workflow.driver.select",
        Some(&root),
        json!({"mode":"agent"}),
    )
    .unwrap();
    for _ in 0..50 {
        drive_manager(&e);
        let state = e.store.snapshot().unwrap();
        if state
            .governance
            .iter()
            .any(|r| r.r#type == "workflow_pending_decision")
            && state.turns.iter().all(|t| !t.status.active())
        {
            break;
        }
    }
    let state = e.store.snapshot().unwrap();
    let request = state
        .governance
        .iter()
        .find(|r| r.r#type == "workflow_pending_decision")
        .unwrap()
        .clone();
    assert_eq!(
        state.turns.len(),
        1,
        "preparation must not require another manager turn"
    );
    assert!(
        !state
            .governance
            .iter()
            .any(|r| r.r#type == "workflow_confirmation")
    );
    cmd(
        &e,
        "workflow.decision.resolve",
        Some(&request.id),
        json!({"approved":true,"reason":"真正人类批准确切原结果"}),
    )
    .unwrap();
    for _ in 0..60 {
        drive_manager(&e);
        if e.store
            .snapshot()
            .unwrap()
            .configuration(&controller)
            .unwrap()
            .data["status"]
            == "closed"
        {
            break;
        }
    }
    let state = e.store.snapshot().unwrap();
    assert_eq!(
        state.configuration(&root).unwrap().data["status"],
        "completed"
    );
    assert_eq!(
        state.configuration(&controller).unwrap().data["status"],
        "closed"
    );
    assert_eq!(
        state.turns.len(),
        1,
        "signature and completion must not create wait-only turns"
    );
    assert!(state.executions.iter().all(|e| e.writer_revoked));
}

#[test]
fn delegated_preparation_preserves_human_gate_and_completes_after_signature_across_restart() {
    let directory = tempfile::tempdir().unwrap();
    let e = engine(directory.path());
    let mut doc = document();
    doc["steps"][0]["confirmation"] = json!("human");
    let root = start(&e, doc);
    for flags in [
        json!({"steps":["one"],"include_human":true,"complete_instance":true}),
        json!({"steps":["one","two"],"prepare_human_requests":true}),
    ] {
        let mut flags = flags;
        flags["reason"] = json!("权限边界反例");
        assert!(cmd(&e, "workflow.segment.dispatch", Some(&root), flags).is_err());
    }
    let grant=cmd(&e,"workflow.segment.dispatch",Some(&root),json!({"steps":["one","two"],"include_human":true,
        "prepare_human_requests":true,"complete_instance":true,"reason":"显式授权准备材料与完整收尾"})).unwrap();
    for _ in 0..10 {
        e.tick(1).unwrap();
    }
    let state = e.store.snapshot().unwrap();
    let requests: Vec<_> = state
        .governance
        .iter()
        .filter(|r| r.r#type == "workflow_pending_decision" && r.definition_id == root)
        .collect();
    assert_eq!(requests.len(), 1);
    let request = requests[0].clone();
    assert_eq!(
        request.data["request"]["operation"],
        "workflow.step.confirm"
    );
    assert_eq!(request.data["actor"]["kind"], "segment");
    assert!(
        !state
            .governance
            .iter()
            .any(|r| r.r#type == "workflow_confirmation")
    );
    assert_eq!(
        runtime(&e, &root)["runtime"]["steps"][1]["status"],
        "queued"
    );
    drop(e);
    let e = engine(directory.path());
    for _ in 0..4 {
        e.tick(1).unwrap();
    }
    assert_eq!(
        e.store
            .snapshot()
            .unwrap()
            .governance
            .iter()
            .filter(|r| r.r#type == "workflow_pending_decision" && r.definition_id == root)
            .count(),
        1
    );
    cmd(
        &e,
        "workflow.decision.resolve",
        Some(&request.id),
        json!({"approved":true,"reason":"真实 Human 核对确切结果"}),
    )
    .unwrap();
    for _ in 0..16 {
        e.tick(1).unwrap();
    }
    let state = e.store.snapshot().unwrap();
    assert_eq!(runtime(&e, &root)["runtime"]["status"], "completed");
    assert_eq!(
        state
            .configuration(grant["id"].as_str().unwrap())
            .unwrap()
            .data["status"],
        "completed"
    );
    let confirmed = state
        .governance
        .iter()
        .find(|r| r.r#type == "workflow_confirmation")
        .unwrap();
    assert_eq!(confirmed.data["actor"]["kind"], "human");
    assert_eq!(
        confirmed.data["outputs"],
        request.data["request"]["arguments"]["outputs"]
    );
    assert_eq!(
        state
            .governance
            .iter()
            .filter(|r| r.r#type == "workflow_completion" && r.definition_id == root)
            .count(),
        1
    );
}

#[test]
fn delegated_preparation_does_not_recreate_a_rejected_request() {
    let directory = tempfile::tempdir().unwrap();
    let e = engine(directory.path());
    let mut doc = document();
    doc["steps"][0]["confirmation"] = json!("human");
    let root = start(&e, doc);
    cmd(
        &e,
        "workflow.segment.dispatch",
        Some(&root),
        json!({"steps":["one","two"],"include_human":true,
        "prepare_human_requests":true,"complete_instance":true,"reason":"拒绝不能重提或代签"}),
    )
    .unwrap();
    for _ in 0..8 {
        e.tick(1).unwrap();
    }
    let state = e.store.snapshot().unwrap();
    let request = state
        .governance
        .iter()
        .find(|r| r.r#type == "workflow_pending_decision" && r.definition_id == root)
        .unwrap();
    cmd(
        &e,
        "workflow.decision.resolve",
        Some(&request.id),
        json!({"approved":false,"reason":"原始材料不满足要求"}),
    )
    .unwrap();
    for _ in 0..10 {
        e.tick(1).unwrap();
    }
    let state = e.store.snapshot().unwrap();
    assert_eq!(
        state
            .governance
            .iter()
            .filter(|r| r.r#type == "workflow_pending_decision" && r.definition_id == root)
            .count(),
        1
    );
    assert!(
        !state
            .governance
            .iter()
            .any(|r| r.r#type == "workflow_confirmation")
    );
    assert_ne!(runtime(&e, &root)["runtime"]["status"], "completed");
    assert_eq!(
        runtime(&e, &root)["runtime"]["steps"][1]["status"],
        "queued"
    );
}

#[test]
fn delegated_preparation_cannot_offer_confirmation_for_failed_completion_checks() {
    let directory = tempfile::tempdir().unwrap();
    let e = engine(directory.path());
    let mut doc = document();
    doc["steps"][0]["confirmation"] = json!("human");
    doc["steps"][0]["completion"] = json!({"op":"equals","source":{"step":"$input","port":"assessment","pointer":"/approve"},"value":true});
    doc["inputs"] =
        json!({"assessment":{"delivery_ref":"fixture-delivery-object:1","required":false}});
    let root = start(&e, doc);
    cmd(&e,"workflow.segment.dispatch",Some(&root),json!({"steps":["one","two"],"include_human":true,
        "prepare_human_requests":true,"complete_instance":true,"reason":"完成校验失败不能准备普通签核"})).unwrap();
    for _ in 0..8 {
        e.tick(1).unwrap();
    }
    let state = e.store.snapshot().unwrap();
    assert!(
        !state
            .governance
            .iter()
            .any(|r| r.r#type == "workflow_pending_decision" && r.definition_id == root)
    );
    assert!(
        !state
            .governance
            .iter()
            .any(|r| r.r#type == "workflow_completion" && r.definition_id == root)
    );
    assert!(
        state
            .governance
            .iter()
            .any(|r| r.r#type == "workflow_segment"
                && r.definition_id == root
                && r.data["status"] == "stopped")
    );
}
