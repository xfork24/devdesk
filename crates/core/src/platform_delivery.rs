//! Frozen source publication, PR preparation, protected merge and local sync.
//! Every physical stage retains its own grant and raw observation.
use crate::{
    concurrency::*,
    governance::{record, update_record},
    platform_facts::{PlatformAction, PlatformPlan, one, restored},
    platform_publish::PublishAction,
    workflow_resources::{WorkflowResources, version},
    *,
};
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) enum DeliveryAction {
    Target {
        context: GitContext,
        expected: String,
        workspace: Id,
        operation: Id,
    },
    Source {
        context: GitContext,
        revision: String,
        operation: Id,
    },
    Open {
        context: GitContext,
        source: String,
        target: String,
        workspace: Id,
        operation: Id,
        delivery: Id,
        state: ResourceSnapshot,
    },
    Merge {
        context: GitContext,
        publication: Box<DeliveryPublication>,
        operation: Id,
        state: ResourceSnapshot,
    },
    Sync {
        context: GitContext,
        publication: Box<DeliveryPublication>,
        receipt: Box<PullRequestReceipt>,
        operation: Id,
    },
}
pub(crate) fn delivery_merge_request(
    context: &GitContext,
    publication: &DeliveryPublication,
) -> Result<PullRequestMergeRequest> {
    Ok(PullRequestMergeRequest {
        request: PullRequestRequest {
            repository: context.workspace.repository.clone(),
            project: context.project.clone(),
            merge: publication
                .pull_request
                .marker
                .split(":merge=")
                .nth(1)
                .and_then(|v| v.strip_suffix(" -->"))
                .ok_or_else(|| err("storage", "PR marker 无效"))?
                .into(),
            head_branch: publication.pull_request.head_branch.clone(),
            base_branch: context.target.branch.clone(),
            candidate: publication.revision.clone(),
            expected_target: publication.target_revision.clone(),
            number: Some(publication.pull_request.number),
        },
        tree: publication.tree.clone(),
    })
}
fn operation(id: &str, context: &GitContext, action: RepositoryAction) -> RepositoryOperation {
    RepositoryOperation {
        id: id.into(),
        project_id: context.project.clone(),
        action,
        status: RepositoryOperationStatus::Dispatched,
        receipt: None,
        reason: String::new(),
        revision: 1,
    }
}
impl Engine {
    fn delivery_step_result(
        &self,
        resources: &WorkflowResources,
        key: &str,
        plan: &mut PlatformPlan,
    ) -> Result<Value> {
        let execution = resources
            .instance
            .steps
            .iter()
            .find(|e| e.step == key)
            .ok_or_else(|| err("storage", "交付政策的步骤不存在"))?;
        require(
            execution.status == StepExecutionStatus::Completed,
            "交付要求的步骤尚未接纳",
        )?;
        let mut selection = crate::workflow_facts::FactSelection::default();
        let source = PortSource {
            step: key.into(),
            port: "result".into(),
            pointer: String::new(),
        };
        selection.source(&source);
        let selected = resources.selected_facts(self.store.as_ref(), &selection)?;
        plan.basis = plan
            .basis
            .merge(selected.reads.versions().iter().cloned())?;
        selected
            .facts
            .value(&source)
            .cloned()
            .ok_or_else(|| err("blocked", "交付缺少已封存结果"))
    }
    fn delivery_test_gate(
        &self,
        resources: &WorkflowResources,
        key: &str,
        revision: &str,
        plan: &mut PlatformPlan,
    ) -> Result<()> {
        let result = self.delivery_step_result(resources, key, plan)?;
        require(
            result["status"] == "succeeded"
                && result["candidate"] == revision
                && result["expected_failure"] == false
                && result["writer_revoked"] == true,
            "正式交付需要确切版本的真实成功测试及停止证明，不能豁免",
        )?;
        let handler=if resources.instance.plan.document.steps.iter().find(|s|s.key==key)
            .is_some_and(|s|matches!(&s.executor, StepExecutor::Tool{tool_ref,..}
                if resources.instance.plan.definitions.values().any(|r|r.id==*tool_ref && r.data["qualified_name"]=="tests.project"))) { "tests.project" } else { "tests.run" };
        plan.basis = plan.basis.merge([self.platform_receipt_proof(
            &resources.instance.root_instance_id,
            handler,
            &result,
        )?])?;
        Ok(())
    }
    pub(crate) fn prepare_delivery_action(
        &self,
        request: &PlatformToolRequest,
        resources: &WorkflowResources,
        plan: &mut PlatformPlan,
    ) -> Result<PlatformAction> {
        let policy = resources
            .instance
            .plan
            .document
            .delivery_policy
            .as_ref()
            .ok_or_else(|| err("configuration_required", "工具需要冻结的交付政策"))?;
        let (context, workspace, reads) = self.platform_native_grant(request, resources)?;
        plan.basis = plan.basis.merge(reads.versions().iter().cloned())?;
        require(
            context.source.owner == request.owner_id
                && context.source.owner != context.target.owner,
            "精简交付需要实际工作归属及上级目标",
        )?;
        let state = one(
            self.store.as_ref(),
            "governance",
            &format!("workflow-delivery:{}", resources.record.id),
        )?;
        plan.basis = plan.basis.merge([version(&state)])?;
        let source = text(&request.arguments["workspace"], "revision")?;
        self.delivery_step_result(resources, &policy.code_review_step, plan)?;
        let review = resources
            .instance
            .steps
            .iter()
            .find(|e| e.step == policy.code_review_step)
            .ok_or_else(|| err("storage", "代码评审执行缺失"))?;
        require(
            review
                .inputs
                .get("workspace")
                .is_some_and(|v| v["revision"] == source),
            "代码评审未绑定当前源版本",
        )?;

        let op = crate::material_commands::stable_object_id(
            self.store.as_ref(),
            &request.execution_id,
            "delivery-operation",
        )?;
        let action = match request.definition.qualified_name.as_str() {
            "workspace.target" => {
                let target = one(
                    self.store.as_ref(),
                    "repository_worktree_owner",
                    &json!([request.project_id, context.target.owner]).to_string(),
                )?;
                plan.basis = plan.basis.merge([version(&target)])?;
                let target: RepositoryWorktree = serde_json::from_value(
                    target
                        .value
                        .ok_or_else(|| err("resource_pending", "上级 worktree 未就绪"))?,
                )
                .map_err(|e| err("storage", e.to_string()))?;
                self.platform_plan_fence(plan, request, &context.target.worktree)?;
                plan.head = Some(one(
                    self.store.as_ref(),
                    "head",
                    &if context.target.owner == context.project {
                        format!("main:{}", context.project)
                    } else {
                        context.target.owner.clone()
                    },
                )?);
                let expected = text(
                    plan.head
                        .as_ref()
                        .unwrap()
                        .value
                        .as_ref()
                        .ok_or_else(|| err("resource_pending", "上级代码头未就绪"))?,
                    "revision",
                )?;
                DeliveryAction::Target {
                    context,
                    expected,
                    workspace: target.id,
                    operation: op,
                }
            }

            "git.source.publish" => {
                self.delivery_test_gate(resources, &policy.source_regression_step, &source, plan)?;
                self.delivery_step_result(resources, &policy.code_review_step, plan)?;
                self.platform_plan_fence(plan, request, &context.source.worktree)?;
                DeliveryAction::Source {
                    context,
                    revision: source,
                    operation: op,
                }
            }
            "git.delivery.open" => {
                let pushed =
                    self.delivery_step_result(resources, &policy.source_publication_step, plan)?;
                require(
                    pushed["revision"] == source && pushed["branch"] == context.source.branch,
                    "PR 创建必须使用已推送的确切开发分支",
                )?;
                plan.basis = plan.basis.merge([self.platform_receipt_proof(
                    &resources.instance.root_instance_id,
                    "git.source.publish",
                    &pushed,
                )?])?;
                self.delivery_test_gate(resources, &policy.source_regression_step, &source, plan)?;
                let target = &request.arguments["target"];
                require(
                    target["owner_id"] == context.target.owner,
                    "PR base 必须为实际工作上级",
                )?;
                let proof = self
                    .platform_receipt_proof(
                        &resources.instance.root_instance_id,
                        "workspace.target",
                        target,
                    )
                    .or_else(|_| {
                        self.platform_receipt_proof(
                            &resources.instance.root_instance_id,
                            "workspace.current",
                            target,
                        )
                    })?;
                plan.basis = plan.basis.merge([proof])?;
                DeliveryAction::Open {
                    context,
                    source,
                    target: text(target, "revision")?,
                    workspace: workspace.id,
                    operation: op,
                    delivery: resources.record.id.clone(),
                    state,
                }
            }
            "git.delivery.merge" => {
                let publication: DeliveryPublication =
                    serde_json::from_value(request.arguments["publication"].clone())
                        .map_err(|e| err("invalid_params", e.to_string()))?;
                require(
                    publication.owner_id == request.owner_id
                        && publication.source_revision == source
                        && publication.source_branch == context.source.branch
                        && publication.pull_request.base_branch == context.target.branch,
                    "PR 项目、源版本或上级归属不符",
                )?;
                plan.basis = plan.basis.merge([self.platform_receipt_proof(
                    &resources.instance.root_instance_id,
                    "git.delivery.open",
                    &json!(publication),
                )?])?;
                let saved = restored(self, &state)?;
                require(
                    saved.data["publication"] == json!(publication),
                    "交付已被后继候选替换",
                )?;
                self.delivery_test_gate(resources, &policy.source_regression_step, &source, plan)?;
                self.delivery_test_gate(
                    resources,
                    &policy.merge_regression_step,
                    &publication.revision,
                    plan,
                )?;
                self.delivery_step_result(resources, &policy.code_review_step, plan)?;
                if context.target.owner == context.project {
                    let key = policy
                        .human_confirmation_step
                        .as_ref()
                        .ok_or_else(|| err("permission_denied", "主分支需要真实 Human 审核"))?;
                    let execution = resources
                        .instance
                        .steps
                        .iter()
                        .find(|e| &e.step == key)
                        .ok_or_else(|| err("storage", "人工审核步骤缺失"))?;
                    require(
                        execution.status == StepExecutionStatus::Completed
                            && execution.inputs.values().any(|v| v == &json!(publication)),
                        "人工审核未绑定当前确切交付",
                    )?;
                    let confirmations =
                        one(self.store.as_ref(), "workflow_confirmations", &execution.id)?;
                    plan.basis = plan.basis.merge([version(&confirmations)])?;
                    require(
                        confirmations
                            .value
                            .as_ref()
                            .and_then(Value::as_array)
                            .is_some_and(|rows| {
                                rows.iter().any(|r| {
                                    r["data"]["actor"]["kind"] == "human"
                                        && r["data"]["input_digest"] == execution.input_digest
                                        && r["data"]["outputs"] == json!(execution.outputs)
                                })
                            }),
                        "缺少不可代签的 Human 决定，不能豁免正式合入",
                    )?;
                    let children = policy.children_step.as_ref().ok_or_else(|| {
                        err("configuration_required", "主分支交付需要当前子流程汇合")
                    })?;
                    self.delivery_step_result(resources, children, plan)?;
                } else {
                    let owner = crate::work_owners::WorkOwnerFacts::read(
                        self.store.as_ref(),
                        &request.owner_id,
                    )?;
                    require(
                        owner.owner.resource.kind == "task"
                            && owner
                                .owner
                                .value
                                .as_ref()
                                .is_some_and(|v| v["feature_id"] == context.target.owner),
                        "自动合入只允许 Task 向实际所属 Feature 交付",
                    )?;
                    plan.basis = plan
                        .basis
                        .merge(owner.read_set.versions().iter().cloned())?;
                    let key = policy.independent_green_step.as_ref().ok_or_else(|| {
                        err("configuration_required", "Task 自动合入需要独立 Green")
                    })?;
                    self.delivery_test_gate(resources, key, &source, plan)?;
                }
                self.platform_plan_fence(plan, request, &context.target.worktree)?;
                plan.head = Some(one(
                    self.store.as_ref(),
                    "head",
                    &if context.target.owner == context.project {
                        format!("main:{}", context.project)
                    } else {
                        context.target.owner.clone()
                    },
                )?);
                if saved.data["synchronized"] == true {
                    let receipt = &saved.data["completion"];
                    self.require_completed_delivery_retry(request, resources, receipt, plan)?;
                    require(
                        self.git.workspace_revision(&context)? == publication.source_revision
                            && plan
                                .head
                                .as_ref()
                                .unwrap()
                                .value
                                .as_ref()
                                .is_some_and(|v| v["revision"] == receipt["revision"])
                            && self.git.delivery_sync_ready(
                                &context,
                                &publication.target_revision,
                                &text(receipt, "revision")?,
                            )?,
                        "原交付已完成，但当前本地目标不再是已核对的同步版本",
                    )?;
                    // A new explicit retry adopts the original physical result.
                    // It never executes status publication, merge or checkout.
                    return Ok(PlatformAction::Value(receipt.clone()));
                }
                if !plan
                    .head
                    .as_ref()
                    .unwrap()
                    .value
                    .as_ref()
                    .is_some_and(|v| v["revision"] == publication.target_revision)
                {
                    return Err(err("stale_target", "本地目标已变化，必须重新回归和审核")
                        .technical_not_applied());
                }
                DeliveryAction::Merge {
                    context,
                    publication: Box::new(publication),
                    operation: op,
                    state,
                }
            }
            _ => return Err(err("unsupported", "未知精简交付动作")),
        };
        Ok(PlatformAction::Delivery(Box::new(action)))
    }
    pub(crate) fn delivery_admission_change(
        &self,
        action: &DeliveryAction,
        receipt: &Value,
    ) -> Result<Option<ResourceChange>> {
        let (state, data) = match action {
            DeliveryAction::Open { state, .. } => {
                (state, json!({"publication":receipt,"synchronized":false}))
            }
            DeliveryAction::Merge {
                state, publication, ..
            } => (
                state,
                json!({"publication":publication,"completion":receipt,"synchronized":true}),
            ),
            _ => return Ok(None),
        };
        let mut row = if state.value.is_some() {
            restored(self, state)?
        } else {
            let mut row = record(
                "workflow_delivery",
                match action {
                    DeliveryAction::Open { delivery, .. } => delivery.clone(),
                    _ => String::new(),
                },
                Scope::project(match action {
                    DeliveryAction::Open { context, .. }
                    | DeliveryAction::Merge { context, .. } => &context.project,
                    _ => unreachable!(),
                }),
                "精简交付状态".into(),
                1,
                None,
                json!({}),
                false,
            );
            row.id = state.resource.id.clone();
            row
        };
        if state.value.is_some() {
            update_record(&mut row, data);
        } else {
            row.data = data;
            row.content_hash = content_digest(&row.refs, &row.data);
        }
        Ok(Some(ResourceChange {
            resource: state.resource.clone(),
            value: row.to_storage(),
        }))
    }
    pub(crate) fn drive_delivery(
        &self,
        effect: &ConfigurationRecord,
        action: &DeliveryAction,
        inspect: bool,
    ) -> Result<Option<Value>> {
        let stage = |name: &str, action: PublishAction| {
            self.platform_publish_stage(effect, name, action, inspect)
        };
        match action {
            DeliveryAction::Target {
                context,
                expected,
                workspace,
                operation: op,
            } => {
                let Some(value) = stage(
                    "target-read",
                    PublishAction::DeliveryTargetHead {
                        context: context.clone(),
                        expected: expected.clone(),
                    },
                )?
                else {
                    return Ok(None);
                };
                let revision = text(&value, "revision")?;
                if &revision != expected {
                    if !self.git.delivery_sync_ready(context, expected, &revision)? {
                        return Ok(None);
                    }
                    let id = crate::material_commands::stable_object_id(
                        self.store.as_ref(),
                        op,
                        "baseline-sync",
                    )?;
                    let sync_action = |operation| PublishAction::DeliverySync {
                        context: context.clone(),
                        expected: expected.clone(),
                        revision: revision.clone(),
                        operation,
                    };
                    let original = stage("baseline-sync", sync_action(id));
                    let sync = match original {
                        Ok(Some(value)) if value["synchronized"] == true => value,
                        _ if inspect => return Ok(None),
                        _ => {
                            let recovery = crate::material_commands::stable_object_id(
                                self.store.as_ref(),
                                op,
                                "baseline-sync-recovery",
                            )?;
                            match stage("baseline-sync-recovery", sync_action(recovery)) {
                                Ok(Some(value)) => value,
                                _ => return Ok(None),
                            }
                        }
                    };
                    if sync["synchronized"] != true {
                        return Ok(None);
                    }
                }
                Ok(Some(
                    json!({"workspace_id":workspace,"revision":revision,"owner_id":context.target.owner,"operation_id":op}),
                ))
            }
            DeliveryAction::Source {
                context,
                revision,
                operation: op,
            } => {
                let operation = operation(
                    op,
                    context,
                    RepositoryAction::PublishRef {
                        merge: op.clone(),
                        context: context.clone(),
                        branch: context.source.branch.clone(),
                        revision: revision.clone(),
                        expected: None,
                        stage: PublishStage::Source,
                    },
                );
                stage("source", PublishAction::Repository(Box::new(operation)))
            }
            DeliveryAction::Open {
                context,
                source,
                target,
                workspace,
                operation: op,
                delivery,
                state,
            } => {
                // New Feature parents may not have a remote ref yet. Mainline
                // refs are never created or advanced by this action.
                if context.target.owner != context.project {
                    let id = crate::material_commands::stable_object_id(
                        self.store.as_ref(),
                        op,
                        "base",
                    )?;
                    let operation = operation(
                        &id,
                        context,
                        RepositoryAction::PublishRef {
                            merge: op.clone(),
                            context: context.clone(),
                            branch: context.target.branch.clone(),
                            revision: target.clone(),
                            expected: None,
                            stage: PublishStage::DeliveryBase,
                        },
                    );
                    if stage("parent-ref", PublishAction::Repository(Box::new(operation)))?
                        .is_none()
                    {
                        return Ok(None);
                    }
                }
                if stage(
                    "protection",
                    PublishAction::DeliveryTarget {
                        context: context.clone(),
                    },
                )?
                .is_none()
                {
                    return Ok(None);
                }
                self.git.fetch_delivery_target(context, target)?;
                let candidate_op = operation(
                    op,
                    context,
                    RepositoryAction::Candidate {
                        merge: op.clone(),
                        context: context.clone(),
                        source: source.clone(),
                        target: target.clone(),
                        session: op.clone(),
                    },
                );
                let Some(value) = stage(
                    "candidate",
                    PublishAction::Repository(Box::new(candidate_op)),
                )?
                else {
                    return Ok(None);
                };
                let RepositoryReceipt::Candidate { revision } = serde_json::from_value(value)
                    .map_err(|e| err("adapter_contract", e.to_string()))?
                else {
                    return Err(err("adapter_contract", "候选回执类型不符"));
                };
                require(
                    self.git.commit_parents(context, &revision)? == vec![target.clone()],
                    "交付候选必须为确切目标的直接子提交",
                )?;
                let previous = state
                    .value
                    .as_ref()
                    .map(|_| restored(self, state))
                    .transpose()?;
                let previous = previous.as_ref().map(|r| &r.data["publication"]);
                let branch = format!("devdesk/candidates/{delivery}");
                let id = crate::material_commands::stable_object_id(
                    self.store.as_ref(),
                    op,
                    "candidate-ref",
                )?;
                let publish = operation(
                    &id,
                    context,
                    RepositoryAction::PublishRef {
                        merge: delivery.clone(),
                        context: context.clone(),
                        branch: branch.clone(),
                        revision: revision.clone(),
                        expected: previous
                            .and_then(|v| v["revision"].as_str())
                            .map(str::to_owned),
                        stage: PublishStage::DeliveryCandidate,
                    },
                );
                if stage(
                    "candidate-ref",
                    PublishAction::Repository(Box::new(publish)),
                )?
                .is_none()
                {
                    return Ok(None);
                }
                let request = PullRequestRequest {
                    repository: context.workspace.repository.clone(),
                    project: context.project.clone(),
                    merge: delivery.clone(),
                    head_branch: branch,
                    base_branch: context.target.branch.clone(),
                    candidate: revision.clone(),
                    expected_target: target.clone(),
                    number: previous.and_then(|v| v["pull_request"]["number"].as_u64()),
                };
                let Some(value) = stage("pr", PublishAction::DeliveryPullRequest(request))? else {
                    return Ok(None);
                };
                let pull_request: PullRequestReceipt = serde_json::from_value(value)
                    .map_err(|e| err("adapter_contract", e.to_string()))?;
                require(
                    pull_request.repository == context.workspace.repository
                        && pull_request.head == revision
                        && pull_request.base_branch == context.target.branch
                        && pull_request.marker
                            == format!(
                                "<!-- devdesk:project={}:merge={} -->",
                                context.project, delivery
                            ),
                    "PR 回执不属于当前冻结交付",
                )?;
                let tree = self.git.commit_tree(context, &revision)?;
                Ok(Some(json!(DeliveryPublication {
                    source_revision: source.clone(),
                    source_branch: context.source.branch.clone(),
                    target_revision: target.clone(),
                    revision,
                    tree,
                    workspace_id: workspace.clone(),
                    owner_id: context.source.owner.clone(),
                    operation_id: op.clone(),
                    pull_request
                })))
            }
            DeliveryAction::Merge {
                context,
                publication,
                operation: op,
                ..
            } => {
                require(
                    self.git.workspace_revision(context)? == publication.source_revision,
                    "源工作版本已变化，旧交付失效",
                )?;
                let request = delivery_merge_request(context, publication)?;
                if stage(
                    "test-status",
                    PublishAction::DeliveryStatus(request.clone()),
                )?
                .is_none()
                {
                    return Ok(None);
                }
                let adapter = self
                    .pull_requests
                    .as_ref()
                    .ok_or_else(|| err("unsupported", "缺少 PR 适配器"))?;
                if !adapter.delivery_merge_ready(&request)? {
                    return Ok(None);
                }
                let Some(value) = stage("merge", PublishAction::DeliveryMerge(request))? else {
                    return Ok(None);
                };
                let receipt: PullRequestReceipt = serde_json::from_value(value)
                    .map_err(|e| err("adapter_contract", e.to_string()))?;
                require(
                    receipt.merged
                        && receipt.repository == context.workspace.repository
                        && receipt.head == publication.revision
                        && receipt.number == publication.pull_request.number
                        && receipt.base_branch == context.target.branch
                        && receipt.marker == publication.pull_request.marker,
                    "远端合入回执归属或源版本不符",
                )?;
                self.drive_delivery(
                    effect,
                    &DeliveryAction::Sync {
                        context: context.clone(),
                        publication: publication.clone(),
                        receipt: Box::new(receipt),
                        operation: op.clone(),
                    },
                    inspect,
                )
            }
            DeliveryAction::Sync {
                context,
                publication,
                receipt,
                operation: op,
            } => {
                let revision = receipt
                    .merge_commit
                    .as_ref()
                    .ok_or_else(|| err("adapter_contract", "远端缺少合入 SHA"))?;
                let id =
                    crate::material_commands::stable_object_id(self.store.as_ref(), op, "sync")?;
                if !self
                    .git
                    .delivery_sync_ready(context, &publication.target_revision, revision)?
                {
                    return Ok(None);
                }
                let original = stage(
                    "sync",
                    PublishAction::DeliverySync {
                        context: context.clone(),
                        expected: publication.target_revision.clone(),
                        revision: revision.clone(),
                        operation: id,
                    },
                );
                let synchronized = match original {
                    Ok(Some(value)) if value["synchronized"] == true => value,
                    _ if inspect => return Ok(None),
                    _ => {
                        // This is a fresh, once-only checkout/sync authorization
                        // after positive local inspection, never another merge.
                        let recovery = crate::material_commands::stable_object_id(
                            self.store.as_ref(),
                            op,
                            "sync-recovery",
                        )?;
                        match stage(
                            "sync-recovery",
                            PublishAction::DeliverySync {
                                context: context.clone(),
                                expected: publication.target_revision.clone(),
                                revision: revision.clone(),
                                operation: recovery,
                            },
                        ) {
                            Ok(Some(value)) => value,
                            _ => return Ok(None),
                        }
                    }
                };
                require(
                    synchronized["synchronized"] == true && synchronized["revision"] == *revision,
                    "本地同步未完成",
                )?;
                Ok(Some(
                    json!({"revision":revision,"publication":publication,"pull_request":receipt,"synchronized":true,"sync":synchronized,"operation_id":op}),
                ))
            }
        }
    }
}
