//! Repository effects receive a once-only native grant. Their literal raw
//! observations survive conflicts with the original admission versions.
use crate::{
    concurrency::*,
    governance::{record, update_record},
    platform_facts::{one, restored},
    work_owners::WorkOwnerFacts,
    workflow_commits::WorkflowCommit,
    workflow_resources::version,
    *,
};
impl Engine {
    pub(crate) fn prepare_native_worktree_intent(
        &self,
        commit: &mut WorkflowCommit<'_>,
        project: &str,
        owner: &str,
        kind: WorktreeKind,
        base: &str,
    ) -> Result<()> {
        let existing = one(self.store.as_ref(), "repository_worktree_operation", owner)?;
        require(
            existing.value.is_none(),
            "该工作已有原始工作副本操作，请核对原操作",
        )?;
        commit.witness([version(&existing)])?;
        let op = RepositoryOperation {
            id: commit.object_id(&format!("worktree:{owner}"))?,
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
        };
        let key = ResourceKey::new("repository_operation", &op.id);
        commit.witness([ResourceVersion {
            resource: key.clone(),
            revision: 0,
        }])?;
        commit.change(ResourceChange {
            resource: key,
            value: json!(op),
        })
    }
    pub(crate) fn native_repository_command(
        &self,
        method: &str,
        c: &Command,
        fingerprint: &str,
        context: &InvocationContext,
    ) -> Result<Option<Value>> {
        if !matches!(
            method,
            "repository.connect" | "repository.refresh" | "repository.reconcile"
        ) {
            return Ok(None);
        }
        require(
            matches!(context.actor(), WorkflowActor::Human),
            "仓库控制需要实际 Human 来源",
        )?;
        if let Some(receipt) = self.store.replay_commit(&c.command_id, fingerprint)? {
            return Ok(Some(receipt.result));
        }
        let target = c
            .target
            .as_deref()
            .ok_or_else(|| err("invalid_params", "缺少原仓库归属"))?;
        let mut commit = WorkflowCommit::new(self, method, c, context)?;
        if method == "repository.reconcile" {
            let selected = one(self.store.as_ref(), "repository_operation", target)?;
            let mut op: RepositoryOperation = serde_json::from_value(
                selected
                    .value
                    .clone()
                    .ok_or_else(|| err("not_found", "原仓库操作缺失"))?,
            )
            .map_err(|e| err("storage", e.to_string()))?;
            context.require_project(&op.project_id)?;
            require(
                c.expected_revision == Some(selected.revision)
                    && op.status == RepositoryOperationStatus::NeedsReconciliation
                    && op.reason != "reconcile_requested",
                "仓库原版本已变化或无需核对",
            )?;
            op.reason = "reconcile_requested".into();
            op.revision += 1;
            commit.witness([version(&selected)])?;
            commit.change(ResourceChange {
                resource: selected.resource,
                value: json!(op),
            })?;
            return commit.finish(fingerprint, json!({"id":target})).map(Some);
        }
        let owner = WorkOwnerFacts::read(self.store.as_ref(), target)?;
        require(
            owner.owner.resource.kind == "project",
            "仓库需要 Project 归属",
        )?;
        owner.require_revision(c.expected_revision)?;
        context.require_project(&owner.project)?;
        commit.witness(owner.read_set.versions().iter().cloned())?;
        let selected = one(self.store.as_ref(), "repository_project_state", target)?;
        let flags = selected
            .value
            .as_ref()
            .ok_or_else(|| err("storage", "仓库状态缺失"))?;
        commit.witness([version(&selected)])?;
        require(
            flags["pending"] == false && flags["git_pending"] == false,
            "请先完成或核对当前仓库/Git 操作",
        )?;
        let project: Project = serde_json::from_value(owner.owner.value.clone().unwrap())
            .map_err(|e| err("storage", e.to_string()))?;
        let action = if method == "repository.connect" {
            require(
                flags["connect_exists"] == false,
                "仓库登记已有原操作，请核对原操作",
            )?;
            let features = one(self.store.as_ref(), "project_features_empty", target)?;
            require(
                features.value.as_ref().is_some_and(|v| v["empty"] == true),
                "已有 Feature 的项目不能切换仓库",
            )?;
            commit.witness([version(&features)])?;
            let repository = text(&c.payload, "repository")?;
            let parts = repository.split('/').collect::<Vec<_>>();
            require(
                parts.len() == 2
                    && parts.iter().all(|s| {
                        !s.is_empty()
                            && s.bytes()
                                .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
                    }),
                "仓库应为 GitHub owner/repo",
            )?;
            let mainline = opt(&c.payload, "mainline").unwrap_or_else(|| "main".into());
            require(!mainline.is_empty(), "主干分支不可为空")?;
            RepositoryAction::Connect {
                repository,
                mainline,
            }
        } else {
            let worktree = one(
                self.store.as_ref(),
                "repository_worktree_owner",
                &json!([target, target]).to_string(),
            )?;
            let workspace: RepositoryWorktree = serde_json::from_value(
                worktree
                    .value
                    .clone()
                    .ok_or_else(|| err("blocked", "主干工作副本未就绪"))?,
            )
            .map_err(|e| err("storage", e.to_string()))?;
            let head = one(self.store.as_ref(), "head", &format!("main:{target}"))?;
            let expected = text(
                head.value
                    .as_ref()
                    .ok_or_else(|| err("blocked", "原主干代码头缺失"))?,
                "revision",
            )?;
            commit.witness([version(&worktree), version(&head)])?;
            let revision = text(&c.payload, "revision")?;
            require(
                matches!(revision.len(), 40 | 64)
                    && revision.bytes().all(|b| b.is_ascii_hexdigit()),
                "需要原观察到的确切远端 SHA",
            )?;
            let location = CodeLocation {
                owner: target.into(),
                branch: workspace.branch,
                worktree: workspace.path,
            };
            RepositoryAction::RefreshMainline {
                context: GitContext {
                    project: target.into(),
                    workspace: project
                        .workspace_definition
                        .ok_or_else(|| err("blocked", "仓库定义缺失"))?,
                    source: location.clone(),
                    target: location,
                    origin: Origin::External,
                },
                expected,
                revision,
            }
        };
        let op = RepositoryOperation {
            id: commit.object_id("repository-operation")?,
            project_id: target.into(),
            action,
            status: RepositoryOperationStatus::Prepared,
            receipt: None,
            reason: String::new(),
            revision: 1,
        };
        let basis = ReadSet::new(
            commit
                .reads
                .versions()
                .iter()
                .filter(|v| v.resource.kind != "repository_project_state")
                .cloned()
                .collect(),
        )?;
        let mut authorization = record(
            "repository_authorization",
            op.id.clone(),
            Scope::project(target),
            "仓库原始写入资格".into(),
            1,
            None,
            json!({"basis":basis,"actor":context.actor(),"command":c,"action":op.action}),
            true,
        );
        authorization.id = format!("repository-authorization:{}", op.id);
        commit.insert_record(&authorization)?;
        let key = ResourceKey::new("repository_operation", &op.id);
        commit.witness([ResourceVersion {
            resource: key.clone(),
            revision: 0,
        }])?;
        commit.change(ResourceChange {
            resource: key,
            value: json!(op),
        })?;
        commit
            .finish(
                fingerprint,
                json!({"id":op.id,"operation_id":op.id,"accepted":true}),
            )
            .map(Some)
    }
    pub fn prepare_task_worktrees(&self) -> Result<()> {
        let key = "task-worktree-page";
        let after = self
            .runtime_callback_cursors
            .lock()
            .map_err(|e| err("storage", e.to_string()))?
            .get(key)
            .cloned()
            .unwrap_or_default();
        let mut page = one(
            self.store.as_ref(),
            "tasks_need_worktree_page",
            &json!([after]).to_string(),
        )?;
        let mut ids: Vec<Id> = serde_json::from_value(page.value.take().unwrap_or(json!([])))
            .map_err(|e| err("storage", e.to_string()))?;
        if ids.is_empty() && !after.is_empty() {
            ids = serde_json::from_value(
                one(
                    self.store.as_ref(),
                    "tasks_need_worktree_page",
                    &json!([""]).to_string(),
                )?
                .value
                .unwrap_or(json!([])),
            )
            .map_err(|e| err("storage", e.to_string()))?;
        }
        for id in ids {
            self.runtime_callback_cursors
                .lock()
                .map_err(|e| err("storage", e.to_string()))?
                .insert(key.into(), id.clone());
            let owner = WorkOwnerFacts::read(self.store.as_ref(), &id)?;
            let raw = owner.owner.value.as_ref().unwrap();
            if raw["status"] == "completed" || raw["worktree"] != "" {
                continue;
            }
            let mut reads = owner.read_set.clone();
            let mut ready = true;
            for dependency in raw["dependencies"]
                .as_array()
                .ok_or_else(|| err("storage", "Task 依赖缺失"))?
            {
                let selected = one(
                    self.store.as_ref(),
                    "task",
                    dependency
                        .as_str()
                        .ok_or_else(|| err("storage", "Task 依赖 ID 无效"))?,
                )?;
                ready &= selected.value.as_ref().is_some_and(|t| {
                    t["status"] == "completed" && t["feature_id"] == raw["feature_id"]
                });
                reads = reads.merge([version(&selected)])?;
            }
            if !ready {
                continue;
            }
            let head = one(
                self.store.as_ref(),
                "head",
                raw["feature_id"].as_str().unwrap_or(""),
            )?;
            let Some(base) = head
                .value
                .as_ref()
                .and_then(|v| v["revision"].as_str())
                .filter(|s| !s.is_empty())
            else {
                continue;
            };
            reads = reads.merge([version(&head)])?;
            let c = Command {
                command_id: format!("task-worktree:{id}"),
                target: Some(id.clone()),
                expected_revision: Some(owner.owner.revision),
                payload: json!({"base":base}),
            };
            let context = InvocationContext::workflow(
                WorkflowActor::Coordinator {
                    source_id: id.clone(),
                },
                Some(owner.project.clone()),
            );
            let mut commit = WorkflowCommit::new(self, "runtime.task.worktree", &c, &context)?;
            commit.coordinator_origin = Some(json!({"kind":"runtime_coordinator","task_id":id}));
            commit.witness(reads.versions().iter().cloned())?;
            self.prepare_native_worktree_intent(
                &mut commit,
                &owner.project,
                &id,
                WorktreeKind::Task,
                base,
            )?;
            let fingerprint = configuration_hash(&("task-worktree/v1", &id, base));
            match commit.finish(&fingerprint, json!({"id":id})) {
                Err(error)
                    if matches!(error.code.as_str(), "storage_busy" | "revision_conflict") =>
                {
                    continue;
                }
                result => {
                    result?;
                }
            }
        }
        Ok(())
    }
    pub fn tick_repositories(&self) -> Result<()> {
        self.prepare_task_worktrees()?;
        let began = std::time::Instant::now();
        let key = "repository-page";
        let after = self
            .runtime_callback_cursors
            .lock()
            .map_err(|e| err("storage", e.to_string()))?
            .get(key)
            .cloned()
            .unwrap_or_default();
        let mut page = one(
            self.store.as_ref(),
            "repository_pending_page",
            &json!([after]).to_string(),
        )?;
        let mut ids: Vec<Id> = serde_json::from_value(page.value.take().unwrap_or(json!([])))
            .map_err(|e| err("storage", e.to_string()))?;
        if ids.is_empty() && !after.is_empty() {
            ids = serde_json::from_value(
                one(
                    self.store.as_ref(),
                    "repository_pending_page",
                    &json!([""]).to_string(),
                )?
                .value
                .unwrap_or(json!([])),
            )
            .map_err(|e| err("storage", e.to_string()))?;
        }
        for (n, id) in ids.iter().enumerate() {
            if n > 0 && began.elapsed() >= std::time::Duration::from_millis(25) {
                break;
            }
            self.runtime_callback_cursors
                .lock()
                .map_err(|e| err("storage", e.to_string()))?
                .insert(key.into(), id.clone());
            match self.drive_native_repository(id) {
                Err(error)
                    if matches!(
                        error.code.as_str(),
                        "storage_busy" | "revision_conflict" | "resource_pending"
                    ) =>
                {
                    continue;
                }
                result => result?,
            }
        }
        Ok(())
    }
    fn drive_native_repository(&self, id: &str) -> Result<()> {
        let selected = one(self.store.as_ref(), "repository_operation", id)?;
        let op: RepositoryOperation = serde_json::from_value(
            selected
                .value
                .clone()
                .ok_or_else(|| err("not_found", "原仓库操作缺失"))?,
        )
        .map_err(|e| err("storage", e.to_string()))?;
        if op.status == RepositoryOperationStatus::Completed {
            return Ok(());
        }
        let key = ResourceKey::new("governance", format!("repository-effect:{id}"));
        let previous = one(self.store.as_ref(), "governance", &key.id)?;
        let (fresh, effect, mut operation) = if previous.value.is_none() {
            require(
                op.status == RepositoryOperationStatus::Prepared,
                "原仓库操作已有外部尝试，缺少旧意图时不能重发",
            )?;
            let mut basis = ReadSet::default();
            let authorization = one(
                self.store.as_ref(),
                "governance",
                &format!("repository-authorization:{id}"),
            )?;
            if authorization.value.is_some() {
                let original = restored(self, &authorization)?;
                require(
                    original.r#type == "repository_authorization"
                        && original.definition_id == op.id
                        && original.data["action"] == json!(op.action),
                    "仓库操作改变了原授权",
                )?;
                basis = serde_json::from_value(original.data["basis"].clone())
                    .map_err(|e| err("storage", e.to_string()))?;
                basis = basis.merge([version(&authorization)])?;
            }
            let mut additions = vec![];
            let mut qualifiers = vec![];
            match &op.action {
                RepositoryAction::Connect { .. } => {
                    for resource in [
                        ResourceKey::new("head", format!("main:{}", op.project_id)),
                        ResourceKey::new("repository_worktree", &op.project_id),
                    ] {
                        let row = one(self.store.as_ref(), &resource.kind, &resource.id)?;
                        require(row.value.is_none(), "原仓库目标已存在，不能覆盖")?;
                        basis = basis.merge([version(&row)])?;
                    }
                }
                RepositoryAction::CreateWorktree {
                    owner,
                    worktree_kind,
                    ..
                } => {
                    let owner = WorkOwnerFacts::read(self.store.as_ref(), owner)?;
                    owner.require_editable()?;
                    require(
                        owner.project == op.project_id
                            && matches!(
                                (owner.owner.resource.kind.as_str(), worktree_kind),
                                ("feature", WorktreeKind::Feature) | ("task", WorktreeKind::Task)
                            ),
                        "原工作副本归属不符",
                    )?;
                    // Full owner versions authorize the external attempt. Its
                    // receipt depends on identity/scope/editability, not a
                    // concurrent status transition such as draft -> running.
                    qualifiers.extend(owner.read_set.versions().iter().cloned());
                    let eligibility = one(
                        self.store.as_ref(),
                        "repository_owner_eligible",
                        &json!([
                            op.project_id,
                            owner.owner.resource.id,
                            owner.owner.resource.kind,
                            owner.parent.as_ref().map(|p| &p.resource.id)
                        ])
                        .to_string(),
                    )?;
                    require(
                        eligibility
                            .value
                            .as_ref()
                            .is_some_and(|v| v["eligible"] == true),
                        "工作副本原归属已冻结或变化",
                    )?;
                    basis = basis.merge([version(&eligibility)])?.merge(
                        owner
                            .read_set
                            .versions()
                            .iter()
                            .filter(|v| v.resource.kind == "project_live")
                            .cloned(),
                    )?;
                    let workspace = one(
                        self.store.as_ref(),
                        "repository_worktree",
                        &owner.owner.resource.id,
                    )?;
                    require(workspace.value.is_none(), "工作副本已有实际归属")?;
                    basis = basis.merge([version(&workspace)])?;
                    if *worktree_kind == WorktreeKind::Task {
                        let head = one(self.store.as_ref(), "head", &owner.owner.resource.id)?;
                        require(head.value.is_none(), "Task 代码头已存在")?;
                        basis = basis.merge([version(&head)])?;
                    }
                }
                RepositoryAction::RefreshMainline { context, .. } => {
                    let writers = one(self.store.as_ref(), "conversation_writers", &op.project_id)?;
                    if !writers
                        .value
                        .as_ref()
                        .and_then(Value::as_array)
                        .is_some_and(|rows| {
                            rows.iter()
                                .all(|w| w["worktree"] != context.target.worktree)
                        })
                    {
                        return Err(err("resource_pending", "主干仍有活动写者或未核对操作"));
                    }
                    let mut fence = record(
                        "platform_workspace_fence",
                        op.id.clone(),
                        Scope::project(&op.project_id),
                        "主干同步工作副本资格".into(),
                        1,
                        None,
                        json!({"status":"active","worktree":context.target.worktree,"operation_id":op.id,"origin":"kernel_repository"}),
                        false,
                    );
                    fence.id = format!("repository-fence:{}", op.id);
                    let key = ResourceKey::new("governance", &fence.id);
                    additions.push((
                        ResourceVersion {
                            resource: key.clone(),
                            revision: 0,
                        },
                        ResourceChange {
                            resource: key.clone(),
                            value: fence.to_storage(),
                        },
                    ));
                    basis = basis.merge([ResourceVersion {
                        resource: key,
                        revision: 1,
                    }])?;
                    qualifiers.push(version(&writers));
                }
                _ => return Err(err("unsupported", "该仓库写入已由独立平台操作协议接管")),
            }
            let mut dispatched = op.clone();
            dispatched.status = RepositoryOperationStatus::Dispatched;
            dispatched.revision += 1;
            let mut effect = record(
                "repository_effect_intent",
                op.id.clone(),
                Scope::project(&op.project_id),
                "原仓库外部操作固定资格".into(),
                1,
                None,
                json!({"operation":dispatched,"operation_version":{"resource":selected.resource,"revision":selected.revision+1},"basis":basis,"issuer":"kernel_repository"}),
                true,
            );
            effect.id = key.id.clone();
            let mut grant_reads = ReadSet::new(
                basis
                    .versions()
                    .iter()
                    .filter(|v| {
                        !additions
                            .iter()
                            .any(|(_, change)| change.resource == v.resource)
                    })
                    .cloned()
                    .collect(),
            )?
            .merge([version(&selected), version(&previous)])?;
            let mut changes = vec![
                ResourceChange {
                    resource: key,
                    value: self.store.encode_runtime_record(&effect)?,
                },
                ResourceChange {
                    resource: selected.resource.clone(),
                    value: json!(dispatched),
                },
            ];
            grant_reads = grant_reads.merge(qualifiers)?;
            for (guard, change) in additions {
                grant_reads = grant_reads.merge([guard])?;
                changes.push(change);
            }
            let outcome = self.store.submit_changes(&PreparedChangeSet {
                command_id: effect.id.clone(),
                fingerprint: configuration_hash(&effect.data),
                read_set: grant_reads,
                changes,
                result: json!({"id":effect.id}),
                time: now() as i64,
            })?;
            (outcome.applied, effect, dispatched)
        } else {
            let effect = restored(self, &previous)?;
            require(
                effect.r#type == "repository_effect_intent"
                    && effect.immutable
                    && effect.definition_id == op.id,
                "原仓库执行意图缺失或改变",
            )?;
            let original: RepositoryOperation =
                serde_json::from_value(effect.data["operation"].clone())
                    .map_err(|e| err("storage", e.to_string()))?;
            require(
                json!(original.action) == json!(op.action) && original.project_id == op.project_id,
                "仓库重入不能换原操作",
            )?;
            (false, effect, original)
        };
        let observed_key = ResourceKey::new("governance", format!("repository-observation:{id}"));
        let observed = one(self.store.as_ref(), "governance", &observed_key.id)?;
        let result: Result<RepositoryReceipt> = if let Some(raw) = &observed.value {
            serde_json::from_value(
                self.store.restore_runtime_record(raw)?["data"]["result"].clone(),
            )
            .map_err(|e| err("storage", e.to_string()))?
        } else {
            let result = if fresh {
                self.git.repository_operation(&operation)
            } else {
                self.git
                    .inspect_repository_operation(&operation)?
                    .ok_or_else(|| {
                        err(
                            "needs_reconciliation",
                            "原仓库操作结果未知；只核对原身份，不重发",
                        )
                    })
            };
            let mut row = record(
                "repository_effect_observation",
                op.id.clone(),
                effect.scope.clone(),
                "原仓库外部观察".into(),
                1,
                None,
                json!({"effect_id":effect.id,"operation_version":effect.data["operation_version"],"result":result,"issuer":"external_git"}),
                true,
            );
            row.id = observed_key.id.clone();
            self.store.commit_changes(&PreparedChangeSet {
                command_id: observed_key.id.clone(),
                fingerprint: configuration_hash(&row.data),
                read_set: ReadSet::new(vec![version(&observed)])?,
                changes: vec![ResourceChange {
                    resource: observed_key.clone(),
                    value: self.store.encode_runtime_record(&row)?,
                }],
                result: json!({"id":row.id}),
                time: now() as i64,
            })?;
            result
        };
        let positive = if op.status == RepositoryOperationStatus::NeedsReconciliation
            && op.reason == "reconcile_requested"
        {
            let key = ResourceKey::new(
                "governance",
                format!("repository-reconciled:{}:{}", id, op.revision),
            );
            let prior = one(self.store.as_ref(), &key.kind, &key.id)?;
            let inspected: Result<RepositoryReceipt> = if let Some(raw) = &prior.value {
                serde_json::from_value(
                    self.store.restore_runtime_record(raw)?["data"]["result"].clone(),
                )
                .map_err(|e| err("storage", e.to_string()))?
            } else {
                self.git
                    .inspect_repository_operation(&operation)
                    .and_then(|receipt| {
                        if let Some(receipt) = receipt {
                            return Ok(receipt);
                        }
                        if let Err(original_error) = &result
                            && matches!(operation.action, RepositoryAction::Connect { .. })
                        {
                            let basis: ReadSet =
                                serde_json::from_value(effect.data["basis"].clone())
                                    .map_err(|e| err("storage", e.to_string()))?;
                            let keys: Vec<_> = basis
                                .versions()
                                .iter()
                                .map(|v| v.resource.clone())
                                .collect();
                            let current = self.store.read_resources(&keys)?;
                            require(
                                current.len() == keys.len()
                                    && current.iter().all(|row| {
                                        basis
                                            .require(&row.resource)
                                            .is_ok_and(|revision| revision == row.revision)
                                    }),
                                "仓库接入的原授权版本已变化",
                            )?;
                            return self
                                .git
                                .resume_unstarted_connection(&operation)?
                                .ok_or_else(|| original_error.clone());
                        }
                        Err(err(
                            "needs_reconciliation",
                            "原物理操作仍未确认，保留原错误",
                        ))
                    })
            };
            if let Err(error) = &inspected
                && error.code == "resource_pending"
            {
                return Err(error.clone());
            }
            let mut row = record(
                "repository_effect_observation",
                op.id.clone(),
                effect.scope.clone(),
                "显式核对原仓库结果".into(),
                1,
                None,
                json!({"effect_id":effect.id,"original_observation":observed_key.id,"reconciliation_version":version(&selected),"result":inspected}),
                true,
            );
            row.id = key.id.clone();
            if prior.value.is_none() {
                self.store.commit_changes(&PreparedChangeSet {
                    command_id: key.id.clone(),
                    fingerprint: configuration_hash(&row.data),
                    read_set: ReadSet::new(vec![version(&prior), version(&selected)])?,
                    changes: vec![ResourceChange {
                        resource: key.clone(),
                        value: self.store.encode_runtime_record(&row)?,
                    }],
                    result: json!({"id":row.id}),
                    time: now() as i64,
                })?;
            }
            Some((
                ResourceVersion {
                    resource: key,
                    revision: 1,
                },
                inspected,
            ))
        } else {
            None
        };
        let expected: ResourceVersion =
            serde_json::from_value(effect.data["operation_version"].clone())
                .map_err(|e| err("storage", e.to_string()))?;
        let current = if op.status == RepositoryOperationStatus::NeedsReconciliation
            && op.reason == "reconcile_requested"
        {
            version(&selected)
        } else {
            expected
        };
        let observation = positive
            .as_ref()
            .map(|(v, _)| v.clone())
            .unwrap_or(ResourceVersion {
                resource: observed_key,
                revision: 1,
            });
        let actual = positive.map(|(_, r)| r).unwrap_or(result);
        let mut changes = vec![];
        let mut reads: ReadSet = serde_json::from_value(effect.data["basis"].clone())
            .map_err(|e| err("storage", e.to_string()))?;
        reads = reads.merge([
            current.clone(),
            observation.clone(),
            ResourceVersion {
                resource: ResourceKey::new("governance", &effect.id),
                revision: 1,
            },
        ])?;
        match &actual {
            Ok(receipt) => {
                // Raw supplier facts stay immutable even when they violate
                // the original contract. Reject the projection and retain the
                // physical fence for an explicit read-only reconciliation.
                let original_reads = reads.clone();
                let original_change_count = changes.len();
                match self.prepare_repository_receipt(&operation, receipt, &mut reads, &mut changes)
                {
                    Ok(()) => {
                        operation.status = RepositoryOperationStatus::Completed;
                        operation.receipt = Some(receipt.clone());
                        operation.reason.clear();
                    }
                    Err(error) => {
                        reads = original_reads;
                        changes.truncate(original_change_count);
                        operation.status = RepositoryOperationStatus::NeedsReconciliation;
                        operation.reason = format!("receipt_rejected · {error}");
                    }
                }
            }
            Err(error) => {
                operation.status = RepositoryOperationStatus::NeedsReconciliation;
                operation.reason = format!("{} · {}", error.code, error.message);
            }
        }
        operation.revision = current.revision + 1;
        changes.push(ResourceChange {
            resource: current.resource.clone(),
            value: json!(operation),
        });
        let result = self.store.commit_changes(&PreparedChangeSet {
            command_id: format!("repository-admit:{id}:{}", current.revision),
            fingerprint: configuration_hash(&(
                "repository-admit/v1",
                &effect.id,
                &actual,
                current.revision,
            )),
            read_set: reads,
            changes,
            result: json!({"id":id}),
            time: now() as i64,
        });
        match result {
            Err(error) if error.code == "revision_conflict" => {
                let selected = one(self.store.as_ref(), "repository_operation", id)?;
                let mut original: RepositoryOperation = serde_json::from_value(
                    selected
                        .value
                        .clone()
                        .ok_or_else(|| err("not_found", "原仓库操作缺失"))?,
                )
                .map_err(|e| err("storage", e.to_string()))?;
                if original.status != RepositoryOperationStatus::Completed {
                    original.status = RepositoryOperationStatus::NeedsReconciliation;
                    original.reason = "原物理观察已保留，原业务接纳版本冲突".into();
                    original.revision += 1;
                    self.store.commit_changes(&PreparedChangeSet {
                        command_id: format!("repository-conflict:{id}:{}", selected.revision),
                        fingerprint: configuration_hash(&json!(original)),
                        read_set: ReadSet::new(vec![version(&selected), observation])?,
                        changes: vec![ResourceChange {
                            resource: selected.resource,
                            value: json!(original),
                        }],
                        result: json!({"id":id}),
                        time: now() as i64,
                    })?;
                }
            }
            result => {
                result?;
            }
        }
        Ok(())
    }
    fn prepare_repository_receipt(
        &self,
        op: &RepositoryOperation,
        receipt: &RepositoryReceipt,
        reads: &mut ReadSet,
        changes: &mut Vec<ResourceChange>,
    ) -> Result<()> {
        let mut worktree = None;
        match (&op.action, receipt) {
            (
                RepositoryAction::Connect {
                    repository,
                    mainline,
                },
                RepositoryReceipt::Connected {
                    policy,
                    workspace,
                    head,
                    worktree: work,
                },
            ) => {
                require(
                    policy.compatible
                        && policy.repository.eq_ignore_ascii_case(repository)
                        && policy.branch == *mainline
                        && policy.head.as_ref() == Some(head)
                        && workspace.repository == *repository
                        && workspace.mainline == *mainline
                        && workspace.path == format!("{}/repository", self.store.root())
                        && work.owner_id == op.project_id
                        && work.kind == WorktreeKind::Mainline
                        && work.base == *head,
                    "仓库接入回执改变了原请求或策略依据",
                )?;
                let key = ResourceKey::new("project", &op.project_id);
                let selected = one(self.store.as_ref(), &key.kind, &key.id)?;
                require(
                    reads.require(&key)? == selected.revision,
                    "仓库接入原 Project 版本已变化",
                )?;
                let mut next = selected
                    .value
                    .ok_or_else(|| err("not_found", "Project 缺失"))?;
                next["workspace"] = json!(workspace.path);
                next["workspace_definition"] = json!(workspace);
                next["execution_mode"] = json!("live");
                next["revision"] = json!(selected.revision + 1);
                changes.push(ResourceChange {
                    resource: key,
                    value: next,
                });
                let key = ResourceKey::new("head", format!("main:{}", op.project_id));
                changes.push(ResourceChange {
                    resource: key.clone(),
                    value: json!(GitHead {
                        id: key.id,
                        project_id: op.project_id.clone(),
                        revision: head.clone()
                    }),
                });
                worktree = Some(work);
            }
            (
                RepositoryAction::CreateWorktree {
                    owner,
                    worktree_kind,
                    base,
                },
                RepositoryReceipt::Worktree { worktree: work },
            ) => {
                require(
                    work.owner_id == *owner && work.kind == *worktree_kind && work.base == *base,
                    "工作副本回执改变了原请求",
                )?;
                if *worktree_kind == WorktreeKind::Task {
                    let key = ResourceKey::new("task", owner);
                    let selected = one(self.store.as_ref(), &key.kind, &key.id)?;
                    let current = selected
                        .value
                        .as_ref()
                        .ok_or_else(|| err("not_found", "Task 缺失"))?;
                    require(
                        current["worktree"] == "" && current["status"] != "completed",
                        "Task 工作副本已改变或冻结",
                    )?;
                    *reads = reads.merge([version(&selected)])?;
                    let mut next = selected
                        .value
                        .ok_or_else(|| err("not_found", "Task 缺失"))?;
                    next["worktree"] = json!(work.path);
                    if next["status"] == "prepared" {
                        next["status"] = json!("queued");
                    }
                    next["revision"] = json!(selected.revision + 1);
                    changes.push(ResourceChange {
                        resource: key,
                        value: next,
                    });
                    let key = ResourceKey::new("head", owner);
                    changes.push(ResourceChange {
                        resource: key.clone(),
                        value: json!(GitHead {
                            id: key.id,
                            project_id: op.project_id.clone(),
                            revision: base.clone()
                        }),
                    });
                }
                worktree = Some(work);
            }
            (
                RepositoryAction::RefreshMainline {
                    context,
                    expected,
                    revision,
                },
                RepositoryReceipt::MainlineRefreshed {
                    previous,
                    revision: actual,
                },
            ) => {
                require(
                    context.project == op.project_id && previous == expected && actual == revision,
                    "原主干刷新回执不符",
                )?;
                let key = ResourceKey::new("head", format!("main:{}", op.project_id));
                changes.push(ResourceChange {
                    resource: key.clone(),
                    value: json!(GitHead {
                        id: key.id,
                        project_id: op.project_id.clone(),
                        revision: revision.clone()
                    }),
                });
                let selected = one(
                    self.store.as_ref(),
                    "governance",
                    &format!("repository-fence:{}", op.id),
                )?;
                let mut row = restored(self, &selected)?;
                let mut data = row.data.clone();
                data["status"] = json!("closed");
                update_record(&mut row, data);
                changes.push(ResourceChange {
                    resource: selected.resource,
                    value: row.to_storage(),
                });
            }
            _ => return Err(err("adapter_contract", "原仓库操作与外部回执类型不符")),
        }
        if let Some(work) = worktree {
            let folder = match work.kind {
                WorktreeKind::Mainline => "mainlines",
                WorktreeKind::Feature => "features",
                WorktreeKind::Task => "tasks",
                WorktreeKind::Candidate => "candidates",
            };
            require(
                work.id == work.owner_id
                    && work.project_id == op.project_id
                    && work.operation_id == op.id
                    && work.path
                        == format!(
                            "{}/execution/worktrees/{folder}/{}",
                            self.store.root(),
                            work.owner_id
                        )
                    && !work.branch.is_empty(),
                "工作副本物理归属不符",
            )?;
            changes.push(ResourceChange {
                resource: ResourceKey::new("repository_worktree", &work.id),
                value: json!(work),
            });
        }
        Ok(())
    }
}
