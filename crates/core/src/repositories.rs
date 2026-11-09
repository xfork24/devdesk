//! Explicit repository intents. Git and network calls run only after the project transaction commits.
use crate::*;
impl Engine {
    pub(crate) fn repository_command(
        &self,
        method: &str,
        c: &Command,
        s: &mut EngineeringState,
    ) -> Result<Option<Value>> {
        let target = c.target.as_deref().unwrap_or("");
        Ok(Some(match method {
            "repository.refresh" => {
                let project = s
                    .projects
                    .iter()
                    .find(|p| p.id == target)
                    .ok_or_else(|| err("not_found", "Project 不存在"))?;
                require(
                    project.execution_mode == "live"
                        && c.expected_revision == Some(project.revision),
                    "需要当前真实 Project revision",
                )?;
                require(
                    !s.advances.iter().any(|a| {
                        ![AdvanceStatus::Applied, AdvanceStatus::NotApplied].contains(&a.status)
                    }),
                    "先核对在途接纳操作",
                )?;
                require(
                    !s.repository_operations.iter().any(|op| {
                        [
                            RepositoryOperationStatus::Prepared,
                            RepositoryOperationStatus::Dispatched,
                            RepositoryOperationStatus::Observed,
                        ]
                        .contains(&op.status)
                    }),
                    "先等待在途仓库操作结束",
                )?;
                let workspace = project
                    .workspace_definition
                    .clone()
                    .ok_or_else(|| err("blocked", "仓库定位缺失"))?;
                let worktree = s
                    .repository_worktrees
                    .iter()
                    .find(|w| w.kind == WorktreeKind::Mainline && w.owner_id == target)
                    .ok_or_else(|| err("blocked", "主干 worktree 缺失"))?;
                let location = CodeLocation {
                    owner: target.into(),
                    branch: worktree.branch.clone(),
                    worktree: worktree.path.clone(),
                };
                let expected = s
                    .heads
                    .iter()
                    .find(|h| h.id == format!("main:{target}"))
                    .ok_or_else(|| err("blocked", "主干版本缺失"))?
                    .revision
                    .clone();
                let revision = text(&c.payload, "revision")?;
                require(
                    revision.len() == 40 && revision.bytes().all(|c| c.is_ascii_hexdigit()),
                    "需要已观察的远端精确 SHA",
                )?;
                let op = RepositoryOperation {
                    id: id(),
                    project_id: target.into(),
                    action: RepositoryAction::RefreshMainline {
                        context: GitContext {
                            project: target.into(),
                            workspace,
                            source: location.clone(),
                            target: location,
                            origin: Origin::External,
                        },
                        expected,
                        revision,
                    },
                    status: RepositoryOperationStatus::Prepared,
                    receipt: None,
                    reason: String::new(),
                    revision: 1,
                };
                let operation_id = op.id.clone();
                s.repository_operations.push(op);
                json!({"id":operation_id,"operation_id":operation_id})
            }
            "repository.connect" => {
                let project = s
                    .projects
                    .iter()
                    .find(|p| p.id == target)
                    .ok_or_else(|| err("not_found", "Project 不存在"))?;
                require(
                    project.execution_mode == "live"
                        && !s.features.iter().any(|f| f.project_id == target)
                        && s.runs.is_empty(),
                    "请在尚未创建 Feature/Run 的独立项目中登记仓库",
                )?;
                require(
                    !s.repository_operations.iter().any(|op| {
                        op.project_id == target
                            && matches!(op.action, RepositoryAction::Connect { .. })
                    }),
                    "仓库登记已存在，请先核对原操作",
                )?;
                let repository = text(&c.payload, "repository")?;
                let mainline = opt(&c.payload, "mainline").unwrap_or_else(|| "main".into());
                let parts: Vec<_> = repository.split('/').collect();
                require(
                    parts.len() == 2
                        && parts.iter().all(|p| {
                            !p.is_empty()
                                && p.bytes()
                                    .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
                        }),
                    "仓库应为 GitHub owner/repo",
                )?;
                let op = RepositoryOperation {
                    id: id(),
                    project_id: target.into(),
                    action: RepositoryAction::Connect {
                        repository,
                        mainline,
                    },
                    status: RepositoryOperationStatus::Prepared,
                    receipt: None,
                    reason: String::new(),
                    revision: 1,
                };
                let id = op.id.clone();
                s.repository_operations.push(op);
                json!({"id":id,"operation_id":id,"accepted":true})
            }
            "repository.reconcile" => {
                let op = s
                    .repository_operations
                    .iter_mut()
                    .find(|op| op.id == target)
                    .ok_or_else(|| err("not_found", "仓库操作不存在"))?;
                require(
                    c.expected_revision == Some(op.revision)
                        && op.status == RepositoryOperationStatus::NeedsReconciliation,
                    "仓库操作无需核对或 revision 已变化",
                )?;
                require(
                    op.reason != "superseded_target",
                    "目标已快进，该旧候选不再重放，请重新准备候选",
                )?;
                op.reason = "reconcile_requested".into();
                op.revision += 1;
                json!({"id":target})
            }
            _ => return Ok(None),
        }))
    }
    pub fn tick_repositories(&self) -> Result<()> {
        let snapshot = self.store.snapshot()?;
        for operation in snapshot.repository_operations.iter().filter(|op| {
            (op.status == RepositoryOperationStatus::Prepared
                && op
                    .reason
                    .strip_prefix("poll_after:")
                    .and_then(|v| v.parse::<u64>().ok())
                    .is_none_or(|v| now() >= v))
                || op.status == RepositoryOperationStatus::Observed
                || (op.status == RepositoryOperationStatus::NeedsReconciliation
                    && op.reason == "reconcile_requested")
        }) {
            self.store.transact(None, &mut |s| {
                let op = s
                    .repository_operations
                    .iter_mut()
                    .find(|op| op.id == operation.id)
                    .unwrap();
                op.status = RepositoryOperationStatus::Dispatched;
                op.revision += 1;
                event(
                    s,
                    &operation.id,
                    "repository.dispatched",
                    "owned repository operation",
                );
                Ok(json!({"ok":true}))
            })?;
            let observed =
                operation
                    .receipt
                    .clone()
                    .map(Ok)
                    .unwrap_or_else(|| match &operation.action {
                        RepositoryAction::PullRequest {
                            request,
                            observe_only,
                            ..
                        } => {
                            let adapter = self
                                .pull_requests
                                .as_ref()
                                .ok_or_else(|| err("unsupported", "PullRequests 未装配"))?;
                            let receipt = if *observe_only {
                                adapter.read(request)?
                            } else {
                                adapter.ensure(request)?
                            };
                            Ok(RepositoryReceipt::PullRequest {
                                pull_request: receipt,
                            })
                        }
                        _ => self.git.repository_operation(operation),
                    });
            if let Ok(receipt) = &observed {
                self.store.transact(None, &mut |s| {
                    let op = s
                        .repository_operations
                        .iter_mut()
                        .find(|op| op.id == operation.id)
                        .unwrap();
                    op.receipt = Some(receipt.clone());
                    op.status = RepositoryOperationStatus::Observed;
                    op.revision += 1;
                    Ok(json!({"ok":true}))
                })?;
            }
            let projected = self.store.transact(None, &mut |s| {
                match &observed {
                    Ok(receipt) => {
                        if let Some(done) = self.project_live_repository(s, operation, receipt)? {
                            let op = s
                                .repository_operations
                                .iter_mut()
                                .find(|op| op.id == operation.id)
                                .unwrap();
                            if done {
                                op.status = RepositoryOperationStatus::Completed;
                                op.receipt = Some(receipt.clone());
                                op.reason.clear();
                            } else if op.revision > 100 {
                                op.status = RepositoryOperationStatus::NeedsReconciliation;
                                op.receipt = None;
                                op.reason = "github_merge_not_observed".into();
                            } else {
                                op.status = RepositoryOperationStatus::Prepared;
                                op.receipt = None;
                                op.reason = format!("poll_after:{}", now() + 5000);
                            }
                            op.revision += 1;
                            return Ok(json!({"ok":true}));
                        }
                        let worktree = match (&operation.action, receipt) {
                            (
                                RepositoryAction::RefreshMainline {
                                    context,
                                    expected,
                                    revision,
                                },
                                RepositoryReceipt::MainlineRefreshed {
                                    previous,
                                    revision: observed,
                                },
                            ) => {
                                require(
                                    context.project == operation.project_id
                                        && previous == expected
                                        && observed == revision,
                                    "主干同步回执不匹配",
                                )?;
                                let head = s
                                    .heads
                                    .iter_mut()
                                    .find(|h| h.id == format!("main:{}", operation.project_id))
                                    .ok_or_else(|| err("blocked", "主干版本缺失"))?;
                                require(
                                    head.revision == *expected || head.revision == *revision,
                                    "同步期间主干事实已变化",
                                )?;
                                head.revision = revision.clone();
                                let mut superseded = vec![];
                                for merge in s.merges.iter_mut().filter(|m| {
                                    m.target == format!("main:{}", operation.project_id)
                                        && m.expected_target != *revision
                                        && m.status != Status::Completed
                                }) {
                                    merge.status = Status::Interrupted;
                                    merge.revision += 1;
                                    superseded.push(merge.id.clone());
                                }
                                for delivery in s.deliveries.iter_mut().filter(|d| {
                                    d.merge_id
                                        .as_ref()
                                        .is_some_and(|id| superseded.contains(id))
                                }) {
                                    delivery.stale = true;
                                }
                                for op in &mut s.repository_operations {
                                    let merge = match &op.action {
                                        RepositoryAction::Candidate { merge, .. }
                                        | RepositoryAction::PublishRef { merge, .. }
                                        | RepositoryAction::PullRequest { merge, .. } => {
                                            Some(merge)
                                        }
                                        _ => None,
                                    };
                                    if op.status != RepositoryOperationStatus::Completed
                                        && merge.is_some_and(|id| superseded.contains(id))
                                    {
                                        op.status = RepositoryOperationStatus::NeedsReconciliation;
                                        op.reason = "superseded_target".into();
                                        op.revision += 1;
                                    }
                                }
                                for r in s.governance.iter_mut().filter(|r| {
                                    r.kind == "workflow_instance"
                                        && r.data["status"] != "completed"
                                        && r.data["status"] != "cancelled"
                                }) {
                                    r.data["dirty"] = json!(true);
                                    r.content_hash = configuration_hash(&r.data);
                                    r.revision += 1;
                                }
                                None
                            }
                            (
                                RepositoryAction::Connect {
                                    repository,
                                    mainline,
                                },
                                RepositoryReceipt::Connected {
                                    policy,
                                    workspace,
                                    head,
                                    worktree,
                                },
                            ) => {
                                require(
                                    policy.compatible
                                        && policy.repository.eq_ignore_ascii_case(repository)
                                        && policy.branch == *mainline
                                        && policy.head.as_ref() == Some(head),
                                    "GitHub 策略/远端头与仓库回执不一致",
                                )?;
                                require(
                                    workspace.path == format!("{}/repository", self.store.root())
                                        && workspace.repository == *repository
                                        && workspace.mainline == *mainline
                                        && worktree.kind == WorktreeKind::Mainline
                                        && worktree.owner_id == operation.project_id
                                        && worktree.base == *head,
                                    "仓库回执与原请求不符",
                                )?;
                                let project = s
                                    .projects
                                    .iter_mut()
                                    .find(|p| p.id == operation.project_id)
                                    .unwrap();
                                project.workspace = workspace.path.clone();
                                project.workspace_definition = Some(workspace.clone());
                                project.execution_mode = "live".into();
                                project.revision += 1;
                                let head_id = format!("main:{}", operation.project_id);
                                if let Some(row) = s.heads.iter_mut().find(|h| h.id == head_id) {
                                    row.revision = head.clone();
                                } else {
                                    s.heads.push(GitHead {
                                        id: head_id,
                                        project_id: operation.project_id.clone(),
                                        revision: head.clone(),
                                    });
                                }
                                self.attach_builtin_project_defaults(s, &operation.project_id)?;
                                Some(worktree)
                            }
                            (
                                RepositoryAction::CreateWorktree {
                                    owner,
                                    worktree_kind,
                                    base,
                                },
                                RepositoryReceipt::Worktree { worktree },
                            ) => {
                                require(
                                    worktree.owner_id == *owner
                                        && worktree.kind == *worktree_kind
                                        && worktree.base == *base,
                                    "worktree 回执与冻结请求不符",
                                )?;
                                Some(worktree)
                            }
                            (
                                RepositoryAction::Snapshot {
                                    run,
                                    context,
                                    expected,
                                    ..
                                },
                                RepositoryReceipt::Snapshot { revision },
                            ) => {
                                require(
                                    run == &operation.id
                                        && context.project == operation.project_id
                                        && revision.len() == expected.len(),
                                    "代码封存回执身份不符",
                                )?;
                                let r = s
                                    .runs
                                    .iter()
                                    .find(|r| &r.id == run)
                                    .ok_or_else(|| err("blocked", "封存源 Run 不存在"))?
                                    .clone();
                                require(
                                    r.origin == Origin::External
                                        && r.writes
                                        && r.worktree == context.source.worktree,
                                    "代码封存源 Run 绑定不符",
                                )?;
                                let head = s
                                    .heads
                                    .iter_mut()
                                    .find(|h| h.id == context.source.owner)
                                    .ok_or_else(|| err("blocked", "封存代码头不存在"))?;
                                require(
                                    head.revision == *expected || head.revision == *revision,
                                    "代码头已变化",
                                )?;
                                head.revision = revision.clone();
                                if r.node == "test_author"
                                    && r.attempt_id.as_deref().is_some_and(|id| {
                                        s.attempt(id).is_ok_and(|a| a.phase == "implementation")
                                    })
                                {
                                    let a = s
                                        .attempts
                                        .iter_mut()
                                        .find(|a| Some(&a.id) == r.attempt_id.as_ref())
                                        .unwrap();
                                    require(
                                        a.baseline.is_none()
                                            || a.baseline.as_ref() == Some(revision),
                                        "Red baseline 已冻结",
                                    )?;
                                    a.baseline = Some(revision.clone());
                                }
                                None
                            }
                            _ => return Err(err("adapter_contract", "仓库回执类型不符")),
                        };
                        if let Some(worktree) = worktree {
                            let folder = match worktree.kind {
                                WorktreeKind::Mainline => "mainlines",
                                WorktreeKind::Feature => "features",
                                WorktreeKind::Task => "tasks",
                                WorktreeKind::Candidate => "candidates",
                            };
                            require(
                                worktree.path
                                    == format!(
                                        "{}/execution/worktrees/{folder}/{}",
                                        self.store.root(),
                                        worktree.owner_id
                                    )
                                    && worktree.id == worktree.owner_id,
                                "worktree 回执路径不属于当前项目协调器",
                            )?;
                            require(
                                worktree.project_id == operation.project_id
                                    && worktree.operation_id == operation.id
                                    && !worktree.path.is_empty()
                                    && !worktree.branch.is_empty(),
                                "worktree 回执归属不符",
                            )?;
                            if worktree.kind == WorktreeKind::Task
                                && !s.heads.iter().any(|h| h.id == worktree.owner_id)
                            {
                                s.heads.push(GitHead {
                                    id: worktree.owner_id.clone(),
                                    project_id: operation.project_id.clone(),
                                    revision: worktree.base.clone(),
                                });
                            }
                            if let Some(task) =
                                s.tasks.iter_mut().find(|t| t.id == worktree.owner_id)
                            {
                                task.worktree = worktree.path.clone();
                                if task.status == Status::Prepared {
                                    task.status = Status::Ready;
                                    task.revision += 1;
                                }
                            }
                            s.repository_worktrees.push(worktree.clone());
                        }
                        let op = s
                            .repository_operations
                            .iter_mut()
                            .find(|op| op.id == operation.id)
                            .unwrap();
                        op.status = RepositoryOperationStatus::Completed;
                        op.receipt = Some(receipt.clone());
                        op.reason.clear();
                        op.revision += 1;
                        event(
                            s,
                            &operation.id,
                            "repository.completed",
                            "provider receipt accepted",
                        );
                    }
                    Err(error) => {
                        let op = s
                            .repository_operations
                            .iter_mut()
                            .find(|op| op.id == operation.id)
                            .unwrap();
                        op.status = RepositoryOperationStatus::NeedsReconciliation;
                        op.reason = format!("{} · {}", error.code, error.message);
                        op.revision += 1;
                        event(
                            s,
                            &operation.id,
                            "repository.needs_reconciliation",
                            &format!("{} · {}", error.code, error.message),
                        );
                    }
                }
                Ok(json!({"ok":true}))
            });
            if let Err(error) = projected {
                self.store.transact(None, &mut |s| {
                    let op = s
                        .repository_operations
                        .iter_mut()
                        .find(|op| op.id == operation.id)
                        .unwrap();
                    op.status = RepositoryOperationStatus::NeedsReconciliation;
                    op.reason = format!("{} · {}", error.code, error.message);
                    op.revision += 1;
                    Ok(json!({"ok":true}))
                })?;
            }
        }
        Ok(())
    }
    pub(crate) fn enqueue_worktree(
        &self,
        s: &mut EngineeringState,
        project: &str,
        owner: &str,
        kind: WorktreeKind,
        base: &str,
    ) {
        s.repository_operations.push(RepositoryOperation {
            id: id(),
            project_id: project.into(),
            action: RepositoryAction::CreateWorktree {
                owner: owner.into(),
                worktree_kind: kind,
                base: base.into(),
            },
            status: RepositoryOperationStatus::Prepared,
            receipt: None,
            reason: String::new(),
            revision: 1,
        });
    }
}
