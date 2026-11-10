//! Native merge preparation and remote delivery are explicit, replayable repository operations.
use crate::*;
fn enqueue(s: &mut EngineeringState, project: &str, action: RepositoryAction) {
    s.repository_operations.push(RepositoryOperation {
        id: id(),
        project_id: project.into(),
        action,
        status: RepositoryOperationStatus::Prepared,
        receipt: None,
        reason: String::new(),
        revision: 1,
    });
}
impl Engine {
    pub(crate) fn prepare_live_merge(
        &self,
        s: &mut EngineeringState,
        c: &Command,
    ) -> Result<Value> {
        let owner = c.target.as_deref().unwrap_or("");
        let (fid, tid) = if let Some(t) = s.tasks.iter().find(|v| v.id == owner) {
            require(
                [Status::Accepted, Status::Merging].contains(&t.status),
                "Task 尚未接受交付",
            )?;
            (t.feature_id.clone(), Some(t.id.clone()))
        } else {
            let f = s.feature(owner)?;
            require(
                [Status::Accepted, Status::Merging].contains(&f.status),
                "Feature 收尾尚未接受",
            )?;
            (f.id.clone(), None)
        };
        mutable_feature(s, &fid)?;
        let feature = s.feature(&fid)?.clone();
        let target = if tid.is_some() {
            fid.clone()
        } else {
            format!("main:{}", feature.project_id)
        };
        let expected = s
            .heads
            .iter()
            .find(|h| h.id == target)
            .ok_or_else(|| err("blocked", "目标头不存在"))?
            .revision
            .clone();
        require(
            !s.merges.iter().any(|m| {
                m.feature_id == fid
                    && m.task_id == tid
                    && [
                        Status::Prepared,
                        Status::Running,
                        Status::WaitingReview,
                        Status::Accepted,
                    ]
                    .contains(&m.status)
                    && m.expected_target == expected
                    && m.input_version == feature.input_version
                    && !s
                        .deliveries
                        .iter()
                        .any(|d| d.merge_id.as_ref() == Some(&m.id) && d.stale)
            }),
            "已有有效候选",
        )?;
        let delivery = s
            .deliveries
            .iter()
            .rev()
            .find(|d| {
                d.feature_id == fid
                    && d.task_id == tid
                    && d.kind
                        == if tid.is_some() {
                            "implementation"
                        } else {
                            "closure"
                        }
                    && s.accepted(d)
            })
            .ok_or_else(|| err("blocked", "缺少已接受源交付"))?
            .clone();
        s.validate_evidence(&delivery)?;
        let session = text(&c.payload, "merge_session")?;
        let role = s
            .sessions
            .iter()
            .find(|v| v.id == session)
            .ok_or_else(|| err("blocked", "候选验证 Session 不存在"))?;
        let role = crate::agent_tasks::session_for_node(
            s,
            role,
            None,
            c.payload["workflow_step_instance_id"].as_str(),
        )?;
        require(
            role.owner_id == owner
                && role
                    .runtime_binding
                    .as_ref()
                    .is_some_and(|b| b.tool_policy == ToolPolicy::ReadOnly),
            "候选验证 Session 范围或权限不符",
        )?;
        let aid = delivery
            .attempt_id
            .as_deref()
            .ok_or_else(|| err("blocked", "源 Attempt 缺失"))?;
        let context = s
            .attempt(aid)?
            .git_context
            .clone()
            .ok_or_else(|| err("blocked", "候选缺少冻结 GitContext"))?;
        let source = delivery
            .candidate
            .clone()
            .ok_or_else(|| err("blocked", "源交付缺少代码对象"))?;
        let mid = id();
        s.merges.push(Merge {
            id: mid.clone(),
            feature_id: fid.clone(),
            task_id: tid.clone(),
            status: Status::Prepared,
            revision: 1,
            input_version: feature.input_version,
            source: source.clone(),
            source_delivery: delivery.id,
            target,
            expected_target: expected.clone(),
            candidate: String::new(),
            committed_revision: None,
            change_request: None,
        });
        s.repository_operations.push(RepositoryOperation {
            id: mid.clone(),
            project_id: feature.project_id,
            action: RepositoryAction::Candidate {
                merge: mid.clone(),
                context,
                source,
                target: expected,
                session,
            },
            status: RepositoryOperationStatus::Prepared,
            receipt: None,
            reason: String::new(),
            revision: 1,
        });
        if let Some(tid) = tid {
            let t = s.tasks.iter_mut().find(|t| t.id == tid).unwrap();
            t.status = Status::Merging;
            t.revision += 1;
        } else {
            let f = s.features.iter_mut().find(|f| f.id == fid).unwrap();
            f.status = Status::Merging;
            f.revision += 1;
        }
        Ok(json!({"id":mid,"operation_id":mid,"status":"prepared"}))
    }
    pub(crate) fn project_live_repository(
        &self,
        s: &mut EngineeringState,
        op: &RepositoryOperation,
        receipt: &RepositoryReceipt,
    ) -> Result<Option<bool>> {
        match (&op.action, receipt) {
            (
                RepositoryAction::Candidate {
                    merge,
                    context,
                    source,
                    target,
                    session,
                },
                RepositoryReceipt::Candidate { revision },
            ) => {
                let m = s.merge(merge)?.clone();
                require(
                    m.source == *source
                        && m.expected_target == *target
                        && m.input_version == s.feature(&m.feature_id)?.input_version
                        && s.heads
                            .iter()
                            .any(|h| h.id == m.target && h.revision == *target),
                    "候选基线已失效",
                )?;
                require(
                    revision.len() == source.len() && !revision.is_empty(),
                    "候选代码回执无效",
                )?;
                let source_delivery = s.delivery(&m.source_delivery)?;
                let standard = s
                    .attempt(source_delivery.attempt_id.as_deref().unwrap())?
                    .binding
                    .standard
                    .clone();
                let binding = self.live_binding(s, session, &standard, None)?;
                let mm = s.merges.iter_mut().find(|v| v.id == *merge).unwrap();
                mm.candidate = revision.clone();
                mm.status = Status::Running;
                mm.revision += 1;
                s.runs.push(Run {
                    id: id(),
                    attempt_id: None,
                    merge_id: Some(merge.clone()),
                    node: "merge_verify".into(),
                    dependencies: vec![],
                    status: Status::Queued,
                    retry_of: None,
                    session_id: session.clone(),
                    binding,
                    binding_verified: true,
                    writes: false,
                    worktree: "candidate-readonly".into(),
                    outcome: "external".into(),
                    cancel_requested: false,
                    started: None,
                    finished: None,
                    result: None,
                    result_artifact: None,
                    origin: Origin::External,
                });
                self.enqueue_worktree(s, &op.project_id, merge, WorktreeKind::Candidate, revision);
                enqueue(
                    s,
                    &op.project_id,
                    RepositoryAction::PublishRef {
                        merge: merge.clone(),
                        context: context.clone(),
                        branch: context.target.branch.clone(),
                        revision: target.clone(),
                        expected: None,
                        stage: PublishStage::TargetBase,
                    },
                );
                Ok(Some(true))
            }
            (
                RepositoryAction::PublishRef {
                    merge,
                    context,
                    branch,
                    revision,
                    stage,
                    ..
                },
                RepositoryReceipt::PublishedRef {
                    revision: actual,
                    branch: observed,
                },
            ) => {
                require(
                    actual == revision && observed == branch,
                    "远端 ref 回执与冻结请求不符",
                )?;
                let m = s.merge(merge)?.clone();
                match stage {
                    PublishStage::TargetBase => enqueue(
                        s,
                        &op.project_id,
                        RepositoryAction::PublishRef {
                            merge: merge.clone(),
                            context: context.clone(),
                            branch: format!("devdesk/candidates/{merge}"),
                            revision: m.candidate.clone(),
                            expected: None,
                            stage: PublishStage::Candidate,
                        },
                    ),
                    PublishStage::Candidate => enqueue(
                        s,
                        &op.project_id,
                        RepositoryAction::PullRequest {
                            merge: merge.clone(),
                            request: PullRequestRequest {
                                repository: context.workspace.repository.clone(),
                                project: op.project_id.clone(),
                                merge: merge.clone(),
                                head_branch: branch.clone(),
                                base_branch: context.target.branch.clone(),
                                candidate: revision.clone(),
                                expected_target: m.expected_target.clone(),
                                number: None,
                            },
                            observe_only: false,
                        },
                    ),
                    PublishStage::RemoteAdvance => {
                        let pr = m
                            .change_request
                            .as_ref()
                            .ok_or_else(|| err("blocked", "远端推进缺少 PR"))?;
                        enqueue(
                            s,
                            &op.project_id,
                            RepositoryAction::PullRequest {
                                merge: merge.clone(),
                                request: PullRequestRequest {
                                    repository: context.workspace.repository.clone(),
                                    project: op.project_id.clone(),
                                    merge: merge.clone(),
                                    head_branch: format!("devdesk/candidates/{merge}"),
                                    base_branch: branch.clone(),
                                    candidate: revision.clone(),
                                    expected_target: m.expected_target.clone(),
                                    number: Some(
                                        pr.external_id
                                            .parse()
                                            .map_err(|_| err("blocked", "PR number 无效"))?,
                                    ),
                                },
                                observe_only: true,
                            },
                        );
                    }
                }
                Ok(Some(true))
            }
            (
                RepositoryAction::PullRequest {
                    merge,
                    request,
                    observe_only,
                },
                RepositoryReceipt::PullRequest { pull_request: pr },
            ) => {
                require(
                    pr.repository == request.repository
                        && pr.marker == request.marker()
                        && pr.head == request.candidate
                        && pr.head_branch == request.head_branch
                        && pr.base_branch == request.base_branch
                        && request.number.is_none_or(|v| v == pr.number),
                    "PR 回执与操作意图不符",
                )?;
                let m = s.merge(merge)?.clone();
                require(m.candidate == pr.head, "PR 不属于当前候选")?;
                if !observe_only {
                    let mm = s.merges.iter_mut().find(|m| m.id == *merge).unwrap();
                    mm.change_request = Some(ChangeRequestRef {
                        provider: "github".into(),
                        repository: pr.repository.clone(),
                        external_id: pr.number.to_string(),
                        source_revision: pr.head.clone(),
                        target_revision: request.expected_target.clone(),
                    });
                    mm.revision += 1;
                    return Ok(Some(true));
                }
                if !pr.merged {
                    return Ok(Some(false));
                }
                require(
                    pr.merge_commit.as_deref() == Some(&m.candidate)
                        && m.committed_revision.as_ref() == Some(&m.candidate)
                        && s.advances.iter().any(|a| {
                            a.id == *merge
                                && a.status == AdvanceStatus::Applied
                                && a.receipt.as_ref() == Some(&m.candidate)
                        }),
                    "PR merged 尚未与实际本地接纳候选一致",
                )?;
                let mm = s.merges.iter_mut().find(|v| v.id == *merge).unwrap();
                mm.status = Status::Completed;
                mm.revision += 1;
                if let Some(tid) = &m.task_id {
                    let t = s.tasks.iter_mut().find(|t| &t.id == tid).unwrap();
                    t.status = Status::Completed;
                    t.revision += 1;
                } else {
                    let f = s
                        .features
                        .iter_mut()
                        .find(|f| f.id == m.feature_id)
                        .unwrap();
                    f.status = Status::Completed;
                    f.revision += 1;
                }
                let delivery = s
                    .deliveries
                    .iter()
                    .find(|d| d.merge_id.as_ref() == Some(merge))
                    .ok_or_else(|| err("blocked", "候选交付缺失"))?
                    .clone();
                let source = s.delivery(&m.source_delivery)?;
                let binding = s
                    .attempt(source.attempt_id.as_deref().unwrap())?
                    .binding
                    .clone();
                self.snapshot(
                    s,
                    &m.feature_id,
                    m.task_id.as_deref(),
                    "merged",
                    binding,
                    Some(delivery.id),
                )?;
                event(
                    s,
                    merge,
                    "merge.remote_completed",
                    &format!("PR {} merged; candidate={}", pr.number, m.candidate),
                );
                Ok(Some(true))
            }
            _ => Ok(None),
        }
    }
    pub(crate) fn enqueue_remote_advance(
        &self,
        s: &mut EngineeringState,
        m: &Merge,
        context: &GitContext,
    ) {
        enqueue(
            s,
            &context.project,
            RepositoryAction::PublishRef {
                merge: m.id.clone(),
                context: context.clone(),
                branch: context.target.branch.clone(),
                revision: m.candidate.clone(),
                expected: Some(m.expected_target.clone()),
                stage: PublishStage::RemoteAdvance,
            },
        );
    }
}
