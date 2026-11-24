//! Public workflow controls retain their original draft and native versions.
use crate::{
    concurrency::*,
    governance::record,
    material_commands::stable_object_id,
    platform_facts::{one, restored},
    workflow_commits::{WorkflowCommit, WorkflowHeaderFacts},
    workflow_facts::FactSelection,
    workflow_resources::{WorkflowResources, version},
    *,
};

impl Engine {
    pub(crate) fn native_workflow_command(
        &self,
        method: &str,
        c: &Command,
        fingerprint: &str,
        context: &InvocationContext,
    ) -> Result<Option<Value>> {
        if method == "workflow.delivery.republish" {
            return self
                .native_delivery_republish(c, fingerprint, context)
                .map(Some);
        }
        if method == "workflow.segment.dispatch" {
            return self
                .native_segment_dispatch(c, fingerprint, context)
                .map(Some);
        }
        if method == "workflow.driver.select" {
            return self.native_driver_select(c, fingerprint, context).map(Some);
        }
        if method == "workflow.change.apply" {
            return self
                .native_workflow_change_command(c, fingerprint, context)
                .map(Some);
        }
        if method == "workflow.change.reconcile" {
            return self
                .native_workflow_change_reconcile(c, fingerprint, context)
                .map(Some);
        }
        if method == "workflow.decision.reconcile" {
            return self
                .reconcile_material_decision(c, fingerprint, context)
                .map(Some);
        }
        if method == "workflow.workspace.reconcile" {
            return self
                .reconcile_workspace_continuation(c, fingerprint, context)
                .map(Some);
        }
        if method == "workflow.start" {
            return self
                .native_workflow_start(c, fingerprint, context)
                .map(Some);
        }
        if [
            "workflow.execution.stop",
            "workflow.step.input",
            "workflow.step.retry",
            "workflow.step.rework",
            "workflow.step.reconcile",
            "workflow.execution.reconcile",
        ]
        .contains(&method)
        {
            if let Some(receipt) = self.store.replay_commit(&c.command_id, fingerprint)? {
                return Ok(Some(receipt.result));
            }
            return self
                .native_workflow_execution_control(method, c, fingerprint, context)
                .map(Some);
        }
        if method == "workflow.step.dispatch" {
            if let Some(receipt) = self.store.replay_commit(&c.command_id, fingerprint)? {
                return Ok(Some(receipt.result));
            }
            return self
                .native_workflow_dispatch_command(c, fingerprint, context)
                .map(Some);
        }
        if ![
            "workflow.pause",
            "workflow.resume",
            "workflow.cancel",
            "workflow.step.confirm",
            "workflow.step.approve",
            "workflow.step.accept",
            "workflow.materials.import",
            "workflow.decision.request",
            "workflow.decision.resolve",
            "workflow.complete",
        ]
        .contains(&method)
        {
            return Ok(None);
        }
        if method == "workflow.materials.import"
            && let Some(result) = self.replay_sealed_command(&c.command_id, fingerprint)?
        {
            return Ok(Some(result));
        }
        if let Some(receipt) = self.store.replay_commit(&c.command_id, fingerprint)? {
            return Ok(Some(receipt.result));
        }
        if method == "workflow.decision.resolve" {
            return self
                .native_workflow_decision(c, fingerprint, context)
                .map(Some);
        }
        let root = WorkflowHeaderFacts::read(
            self,
            c.target
                .as_deref()
                .ok_or_else(|| err("invalid_params", "缺少流程目标"))?,
        )?;
        let mut commit = WorkflowCommit::new(self, method, c, context)?;
        commit.witness([version(&root.header)])?;
        commit.authorize(&root)?;
        root.active()?;
        if !["workflow.pause", "workflow.cancel"].contains(&method) {
            commit.require_no_change(&root.record.id)?;
        }
        let reason = c.payload["reason"].as_str().unwrap_or(method);
        match method {
            "workflow.pause" | "workflow.resume" | "workflow.cancel" => {
                root.guard_request(c, None)?;
                let mut next = root.instance.as_ref().clone();
                next.control_version += 1;
                let active = one(
                    self.store.as_ref(),
                    "workflow_active_pause",
                    &root.record.id,
                )?;
                commit.witness([version(&active)])?;
                if method == "workflow.pause" {
                    if !next.paused {
                        require(active.value.is_none(), "原暂停区间尚未结束")?;
                        commit.new_record(
                            record(
                                "workflow_pause_interval",
                                root.record.id.clone(),
                                root.record.scope.clone(),
                                reason.into(),
                                1,
                                None,
                                json!({"started":now(),"finished":null,"actor":context.actor()}),
                                false,
                            ),
                            "workflow-pause-interval",
                        )?;
                    }
                    next.paused = true;
                } else {
                    if active.value.is_some() {
                        let row = restored(self, &active)?;
                        let mut data = row.data.clone();
                        data["finished"] = json!(now());
                        commit.replace_record(
                            &ResourceSnapshot {
                                resource: ResourceKey::new("governance", &row.id),
                                revision: active.revision,
                                value: active.value.clone(),
                            },
                            row,
                            data,
                        )?;
                    }
                    next.paused = false;
                    if method == "workflow.cancel" {
                        next.cancel_requested = true;
                    }
                }
                commit.header(&root, &next)?;
                commit.decision(&root, None, method.rsplit('.').next().unwrap(), reason)?;
                commit.finish(fingerprint,json!({"id":root.record.id,"accepted":true,"control_version":next.control_version})).map(Some)
            }
            "workflow.step.confirm" | "workflow.step.approve" | "workflow.step.accept" => {
                let key = c.payload["execution_id"]
                    .as_str()
                    .or_else(|| c.payload["step"].as_str())
                    .ok_or_else(|| err("invalid_params", "缺少原执行"))?;
                let resources = root.step(self, key)?;
                root.guard_request(c, Some(&resources.step))?;
                self.require_execution_time(&resources)?;
                commit.witness(resources.reads.versions().iter().cloned())?;
                let definition = resources
                    .instance
                    .plan
                    .document
                    .steps
                    .iter()
                    .find(|s| s.key == resources.execution.step)
                    .ok_or_else(|| err("storage", "原步骤定义缺失"))?;
                let mut selected = FactSelection::default();
                selected.rule(&definition.completion);
                selected.approval(&definition.key);
                let facts = resources.selected_facts(self.store.as_ref(), &selected)?;
                commit.witness(facts.reads.versions().iter().cloned())?;
                require(
                    c.payload["execution_id"] == resources.execution.id
                        && c.payload["input_digest"] == resources.execution.input_digest
                        && c.payload["outputs"] == json!(resources.execution.outputs),
                    "决定必须绑定原执行、原输入和原产物",
                )?;
                if method == "workflow.step.accept" {
                    commit.require_acceptance(&resources, &facts.facts)?;
                    let (wait_reads, wait_changes) =
                        self.resume_native_workflow_wait(&resources.execution)?;
                    commit.witness(wait_reads.versions().iter().cloned())?;
                    for change in wait_changes {
                        commit.change(change)?;
                    }
                    let mut next = resources.execution.clone();
                    next.status = StepExecutionStatus::Completed;
                    // Also settle legacy reconciled results whose positive
                    // outputs were admitted without clearing the old failure.
                    if let Some(failure) = next.failure.take()
                        && next.reason == failure.message
                    {
                        next.reason = "原执行结果已核对并接纳".into();
                    }
                    commit.step(&resources, &mut next)?;
                    let decision =
                        commit.decision(&root, Some(&resources.execution), "accept", reason)?;
                    commit.finish(fingerprint,json!({"id":root.record.id,"execution":next,"decision_id":decision,"plan_revision_id":root.instance.active_plan_revision_id,"control_version":root.instance.control_version})).map(Some)
                } else {
                    self.prepare_native_confirmation(
                        &mut commit,
                        &resources,
                        &c.payload,
                        method == "workflow.step.approve",
                        &facts.facts,
                    )?;
                    self.settle_matching_step_decisions(&mut commit, &resources, reason, None)?;
                    commit.decision(
                        &root,
                        Some(&resources.execution),
                        if method == "workflow.step.approve" {
                            "approve"
                        } else {
                            "confirm"
                        },
                        reason,
                    )?;
                    commit
                        .finish(fingerprint, json!({"id":root.record.id,"accepted":true}))
                        .map(Some)
                }
            }
            "workflow.materials.import" => {
                root.guard_request(c, None)?;
                let catalog = self.store.configuration_snapshot()?;
                let delivery = catalog
                    .state
                    .configuration(&text(&c.payload, "delivery_ref")?)?;
                require(
                    delivery.r#type == "delivery_type"
                        && delivery.scope.visible_from(&root.record.scope),
                    "交付类型不可用",
                )?;
                let definition: DeliveryTypeDefinition = delivery.decode()?;
                require(
                    !matches!(
                        definition.producer,
                        Some(DeliveryProducer::Platform { .. } | DeliveryProducer::Workflow)
                    ),
                    "平台或子流程产物必须引用实际执行",
                )?;
                let value = c
                    .payload
                    .get("value")
                    .ok_or_else(|| err("invalid_params", "缺少材料正文"))?;
                self.enforce_delivery_value(&catalog.state, delivery, value)?;
                commit.witness([ResourceVersion {
                    resource: ResourceKey::new("catalog_clock", "configuration"),
                    revision: catalog.revision,
                }])?;
                let body = value.to_string();
                let seal = ArtifactSeal::prepare(
                    stable_object_id(
                        self.store.as_ref(),
                        &c.command_id,
                        "workflow-material-artifact",
                    )?,
                    &MaterialOwner::Project {
                        id: root.instance.project_id.clone(),
                    },
                    "materials",
                    &body,
                    Origin::Human,
                )?;
                let material = TypedMaterial {
                    project_id: root.instance.project_id.clone(),
                    root_instance_id: root.instance.root_instance_id.clone(),
                    delivery_ref: delivery.id.clone(),
                    value: value.clone(),
                    value_digest: configuration_hash(value),
                    artifact_id: seal.artifact.id.clone(),
                    source: text(&c.payload, "source")?,
                };
                let id = commit.new_record(
                    record(
                        "typed_material",
                        root.instance.root_instance_id.clone(),
                        root.record.scope.clone(),
                        reason.into(),
                        1,
                        None,
                        json!(material),
                        true,
                    ),
                    "workflow-material",
                )?;
                commit.decision(&root, None, "import", reason)?;
                self.seal_command_changes(
                    commit.prepare(fingerprint, json!({"id":id,"material":material}))?,
                    seal,
                    &body,
                    root.record.scope.clone(),
                )
                .map(Some)
            }
            "workflow.decision.request" => {
                root.guard_request(c, None)?;
                let operation = text(&c.payload, "operation")?;
                text(&c.payload, "reason")?;
                require(
                    crate::controllers::CONTROL_TOOLS.contains(&operation.as_str())
                        || matches!(
                            operation.as_str(),
                            "workflow.step.confirm" | "workflow.step.approve"
                        ),
                    "不是可请求的人类决定",
                )?;
                let arguments = &c.payload["arguments"];
                require(
                    arguments.is_object()
                        && arguments["plan_revision_id"] == root.instance.active_plan_revision_id,
                    "请求必须绑定原计划和确切参数",
                )?;
                if let Some(target) = arguments["instance_id"]
                    .as_str()
                    .or_else(|| arguments["id"].as_str())
                {
                    require(target == root.record.id, "决定目标与流程不一致")?;
                }
                if arguments["execution_id"].is_string()
                    && !matches!(
                        operation.as_str(),
                        "workflow.step.confirm" | "workflow.step.accept" | "workflow.step.approve"
                    )
                {
                    self.require_step_decision(&mut commit, &root, &operation, arguments)?;
                }
                let time_resources = if matches!(
                    operation.as_str(),
                    "workflow.step.confirm" | "workflow.step.accept" | "workflow.step.approve"
                ) {
                    let resources = root.step(self, &text(arguments, "execution_id")?)?;
                    self.require_execution_time(&resources)?;
                    commit.witness(resources.reads.versions().iter().cloned())?;
                    require(
                        matches!(
                            resources.execution.status,
                            StepExecutionStatus::AwaitingAcceptance
                                | StepExecutionStatus::WaitingConfirmation
                        ) && arguments["input_digest"] == resources.execution.input_digest
                            && arguments["outputs"] == json!(resources.execution.outputs),
                        "决定必须绑定当前可接纳的原产物",
                    )?;
                    if matches!(context.actor(), WorkflowActor::Controller { .. })
                        && (self.test_recovery(&mut commit, &resources)?.is_some()
                            || self.review_recovery(&mut commit, &root, &resources)?["status"]
                                == "needs_manager")
                    {
                        return Err(err(
                            "technical_recovery_pending",
                            "当前结果须先走适用的恢复和返工流程",
                        ));
                    }
                    Some(resources)
                } else {
                    None
                };
                let mut request = c.payload.clone();
                if operation == "workflow.step.approve" {
                    let resources = root.step(self, &text(arguments, "execution_id")?)?;
                    require(
                        self.test_recovery(&mut commit, &resources)?.is_none(),
                        "此独立测试失败应由项目经理恢复",
                    )?;
                    request["approval_basis"] =
                        self.native_approval_basis(&mut commit, &resources)?;
                    for field in ["issues", "recommendation"] {
                        text(&request["interaction"], field)?;
                    }
                    // One request per exact result, including requests a human
                    // rejected. A new command ID cannot recreate the same plea.
                    let mut guard = record(
                        "workflow_approval_request",
                        root.record.id.clone(),
                        root.record.scope.clone(),
                        reason.into(),
                        1,
                        None,
                        json!({"basis":request["approval_basis"]}),
                        true,
                    );
                    guard.id = format!(
                        "workflow-approval:{}",
                        configuration_hash(&request["approval_basis"])
                    );
                    commit.insert_record(&guard)?;
                }
                if matches!(context.actor(), WorkflowActor::Controller { .. })
                    || !request["interaction"].is_null()
                {
                    request["interaction"] = self.validate_controller_interaction(
                        &mut commit,
                        &root,
                        &request["interaction"],
                        arguments,
                    )?;
                }
                if operation == "workflow.step.input"
                    && let WorkflowActor::Controller {
                        controller_id,
                        epoch,
                        ..
                    } = context.actor()
                {
                    let resources = root.step(self, &text(arguments, "execution_id")?)?;
                    let reviewed = one(
                        self.store.as_ref(),
                        "controller_execution_review",
                        &json!([controller_id, epoch, root.record.id, resources.execution.id])
                            .to_string(),
                    )?;
                    require(
                        reviewed.value.is_some(),
                        "请先实际读取此执行及已有材料，再请求只有人类可提供的输入",
                    )?;
                    commit.witness([version(&reviewed)])?;
                    require(
                        commit
                            .resolve_material_input(&root, &text(arguments, "input")?)?
                            .is_none(),
                        "已读到的原始平台材料应直接转交，不需要人类签署",
                    )?;
                    require(
                        request["interaction"]["evidence"]
                            .as_array()
                            .is_some_and(|items| {
                                items.iter().any(|item| {
                                    item["execution_id"] == resources.execution.id
                                        && item["instance_id"] == root.record.id
                                })
                            }),
                        "向人类请求事实前，经理必须引用已核对的原执行依据",
                    )?;
                    let decisions = one(
                        self.store.as_ref(),
                        "workflow_step_decisions",
                        &resources.execution.id,
                    )?;
                    commit.witness([version(&decisions)])?;
                    require(
                        !decisions
                            .value
                            .as_ref()
                            .and_then(Value::as_array)
                            .into_iter()
                            .flatten()
                            .any(|row| {
                                row["data"]["request"]["operation"] == "workflow.step.input"
                                    && row["data"]["request"]["arguments"]["expected_step_revision"]
                                        == resources.step.revision
                            }),
                        "此执行已有待处理的补充输入请求",
                    )?;
                }
                if let Some(resources) = time_resources
                    && resources
                        .execution
                        .time_budget
                        .as_ref()
                        .is_some_and(|b| b.human_wait_closed)
                {
                    let approvals = one(
                        self.store.as_ref(),
                        "workflow_confirmations",
                        &resources.execution.id,
                    )?;
                    commit.witness([version(&approvals)])?;
                    let signed = approvals
                        .value
                        .as_ref()
                        .and_then(Value::as_array)
                        .is_some_and(|rows| {
                            rows.iter().any(|row| {
                                row["data"]["input_digest"] == resources.execution.input_digest
                                    && row["data"]["outputs"] == json!(resources.execution.outputs)
                            })
                        });
                    if !signed {
                        let mut next = resources.execution.clone();
                        let budget = next.time_budget.as_mut().unwrap();
                        budget.human_wait_closed = false;
                        budget.human_wait_started_at = None;
                        commit.step(&resources, &mut next)?;
                    }
                }
                let mut pending = record(
                    "workflow_pending_decision",
                    root.record.id.clone(),
                    root.record.scope.clone(),
                    reason.into(),
                    1,
                    None,
                    json!({"status":"pending","plan_revision_id":root.instance.active_plan_revision_id,"request":request,"actor":context.actor()}),
                    false,
                );
                let id = if let WorkflowActor::Segment { .. } = context.actor() {
                    let resources = root.step(self, &text(arguments, "execution_id")?)?;
                    pending.id = crate::workflow_segment_actions::segment_human_request_id(
                        &resources.execution,
                    );
                    let id = pending.id.clone();
                    commit.insert_record(&pending)?;
                    id
                } else {
                    commit.new_record(pending, "workflow-pending-decision")?
                };
                commit
                    .finish(fingerprint, json!({"id":id,"status":"pending"}))
                    .map(Some)
            }
            "workflow.complete" => {
                root.guard_request(c, None)?;
                require(
                    root.instance.steps.iter().all(|e| {
                        matches!(
                            e.status,
                            StepExecutionStatus::Completed | StepExecutionStatus::Skipped
                        )
                    }),
                    "仍有未接纳的步骤",
                )?;
                let first = root
                    .instance
                    .steps
                    .first()
                    .ok_or_else(|| err("storage", "流程没有步骤"))?;
                let resources = root.step(self, &first.id)?;
                let mut selection = FactSelection::default();
                selection.rule(&root.instance.plan.document.completion);
                for source in root.instance.plan.document.outputs.values() {
                    selection.source(source);
                }
                let facts = resources.selected_facts(self.store.as_ref(), &selection)?;
                commit.witness(facts.reads.versions().iter().cloned())?;
                let keys = root
                    .instance
                    .steps
                    .iter()
                    .map(|e| {
                        ResourceKey::new("workflow_step", json!([root.record.id, e.id]).to_string())
                    })
                    .collect::<Vec<_>>();
                let rows = self.store.read_resources(&keys)?;
                require(
                    rows.len() == keys.len()
                        && rows.iter().zip(&keys).all(|(r, k)| {
                            &r.resource == k
                                && r.value.as_ref().is_some_and(|v| {
                                    matches!(v["status"].as_str(), Some("completed" | "skipped"))
                                })
                        }),
                    "原步骤已变化",
                )?;
                commit.witness(rows.iter().map(version))?;
                require(
                    commit.rule(
                        &root.instance,
                        "$workflow",
                        &root.instance.plan.document.completion,
                        &facts.facts,
                    )? && root
                        .instance
                        .plan
                        .document
                        .outputs
                        .values()
                        .all(|source| facts.facts.value(source).is_some()),
                    "整体完成条件或最终交付未满足",
                )?;
                let decision = commit.decision(&root, None, "complete", reason)?;
                let submitted =
                    commit.commit_domain_outputs(&root.instance, &facts.facts, &decision)?;
                commit.record_domain_submission(&root.instance, &submitted)?;
                commit.new_record(record("workflow_completion",root.record.id.clone(),root.record.scope.clone(),reason.into(),1,None,json!({"plan_revision_id":root.instance.active_plan_revision_id,"decision_id":decision}),true),"workflow-completion")?;
                let mut next = root.instance.as_ref().clone();
                if next.plan.document.delivery_policy.is_some() {
                    let delivery = one(
                        self.store.as_ref(),
                        "governance",
                        &format!("workflow-delivery:{}", root.record.id),
                    )?;
                    let saved = crate::platform_facts::restored(self, &delivery)?;
                    require(
                        saved.data["synchronized"] == true
                            && saved.data["completion"]["synchronized"] == true
                            && saved.data["completion"]["pull_request"]["merged"] == true,
                        "精简流程必须在真实 PR 合入并同步后完成",
                    )?;
                    commit.witness([version(&delivery)])?;
                    let task = one(self.store.as_ref(), "task", &next.owner_id)?;
                    if let Some(raw) = &task.value {
                        let mut completed = raw.clone();
                        completed["status"] = json!(Status::Completed);
                        completed["revision"] = json!(task.revision + 1);
                        commit.witness([version(&task)])?;
                        commit.change(ResourceChange {
                            resource: task.resource,
                            value: completed,
                        })?;
                    }
                }
                next.status = StepExecutionStatus::Completed;
                next.control_version += 1;
                commit.header(&root, &next)?;
                if next.parent_execution_id.is_none() {
                    let feature = one(self.store.as_ref(), "feature", &next.owner_id)?;
                    if let Some(raw) = &feature.value {
                        let mut completed = raw.clone();
                        completed["status"] = json!(Status::Completed);
                        completed["revision"] = json!(feature.revision + 1);
                        commit.witness([version(&feature)])?;
                        commit.change(ResourceChange {
                            resource: feature.resource,
                            value: completed,
                        })?;
                    }
                }
                commit
                    .finish(
                        fingerprint,
                        json!({"completed":true,"decision_id":decision}),
                    )
                    .map(Some)
            }
            _ => unreachable!(),
        }
    }
    pub(crate) fn prepare_native_confirmation(
        &self,
        commit: &mut WorkflowCommit<'_>,
        resources: &WorkflowResources,
        payload: &Value,
        manual: bool,
        facts: &WorkflowFacts,
    ) -> Result<()> {
        require(
            matches!(commit.context.actor(), WorkflowActor::Human),
            "人工确认需要实际 Human 来源",
        )?;
        let execution = &resources.execution;
        require(
            matches!(
                execution.status,
                StepExecutionStatus::AwaitingAcceptance | StepExecutionStatus::WaitingConfirmation
            ) && !resources.instance.cancel_requested,
            "原步骤不再等待确认",
        )?;
        require(
            payload["execution_id"] == execution.id
                && payload["input_digest"] == execution.input_digest
                && payload["outputs"] == json!(execution.outputs),
            "确认内容不属于原执行",
        )?;
        let step = resources
            .instance
            .plan
            .document
            .steps
            .iter()
            .find(|s| s.key == execution.step)
            .ok_or_else(|| err("storage", "确认步骤定义缺失"))?;
        let valid = step.completion.evaluate(facts);
        require(
            valid || self.test_recovery(commit, resources)?.is_none(),
            "此独立测试失败需要项目经理恢复，不能作为审批通过",
        )?;
        if manual || !valid {
            text(payload, "reason")?;
        }
        if !valid && !manual {
            require(
                step.allow_completion_override && payload["override_completion"] == true,
                "完成条件未满足且未授权直接通过",
            )?;
        }
        let approvals = one(self.store.as_ref(), "workflow_confirmations", &execution.id)?;
        commit.witness([version(&approvals)])?;
        require(
            !approvals
                .value
                .as_ref()
                .and_then(Value::as_array)
                .is_some_and(|rows| {
                    rows.iter().any(|r| {
                        r["data"]["input_digest"] == execution.input_digest
                            && r["data"]["outputs"] == json!(execution.outputs)
                    })
                }),
            "该确切执行与产物已有确认",
        )?;
        commit.new_record(record("workflow_confirmation",execution.id.clone(),resources.record.scope.clone(),"人工确认".into(),1,None,json!({"execution_id":execution.id,"actor":commit.context.actor(),"input_digest":execution.input_digest,"outputs":execution.outputs,"reason":payload["reason"],"override_completion":!valid,"manual_approval":manual,"approval_basis":payload["approval_basis"],"exception_scope":payload["exception_scope"],"request_id":payload["request_id"]}),true),"workflow-confirmation")?;
        if payload["accept_result"] == true {
            let driver = one(
                self.store.as_ref(),
                "workflow_driver_root",
                &resources.instance.root_instance_id,
            )?;
            commit.witness([version(&driver)])?;
            require(
                driver
                    .value
                    .as_ref()
                    .is_none_or(|row| row["data"]["mode"] != "agent"),
                "Agent 流程的人工决定只能确认；结果必须由项目经理接纳",
            )?;
            require(
                !commit.execution_active(resources)?,
                "执行者或未核对副作用尚未停止",
            )?;
            let (reads, changes) = self.resume_native_workflow_wait(execution)?;
            commit.witness(reads.versions().iter().cloned())?;
            for change in changes {
                commit.change(change)?;
            }
            let mut next = execution.clone();
            next.status = StepExecutionStatus::Completed;
            commit.step(resources, &mut next)?;
            let root = WorkflowHeaderFacts {
                record: resources.record.clone(),
                instance: resources.instance.clone(),
                header: resources.header.clone(),
            };
            commit.decision(
                &root,
                Some(execution),
                "accept",
                payload["reason"]
                    .as_str()
                    .unwrap_or("实际人类确认并接纳当前结果"),
            )?;
        } else if execution.time_budget.is_some() {
            self.close_execution_human_wait(commit, resources)?;
        }

        Ok(())
    }
    fn native_workflow_decision(
        &self,
        c: &Command,
        fingerprint: &str,
        context: &InvocationContext,
    ) -> Result<Value> {
        require(
            matches!(context.actor(), WorkflowActor::Human),
            "只有实际人类可以签署决定",
        )?;
        let selected = one(
            self.store.as_ref(),
            "governance",
            c.target
                .as_deref()
                .ok_or_else(|| err("invalid_params", "缺少决定目标"))?,
        )?;
        let pending = restored(self, &selected)?;
        if c.expected_revision != Some(selected.revision) {
            return Err(err("revision_conflict", "原决定版本已变化"));
        }
        require(
            pending.r#type == "workflow_pending_decision" && pending.data["status"] == "pending",
            "原决定已经结束",
        )?;
        let mut commit = WorkflowCommit::new(self, "workflow.decision.resolve", c, context)?;
        let approved = c.payload["approved"]
            .as_bool()
            .ok_or_else(|| err("invalid_params", "需要明确批准或拒绝"))?;
        let reason = text(&c.payload, "reason")?;
        let result =
            self.prepare_controller_decision(&mut commit, &selected, pending, approved, &reason)?;
        commit.finish(fingerprint, result)
    }
    fn require_step_decision(
        &self,
        commit: &mut WorkflowCommit<'_>,
        root: &WorkflowHeaderFacts,
        operation: &str,
        arguments: &Value,
    ) -> Result<()> {
        let resources = root.step(self, &text(arguments, "execution_id")?)?;
        require(
            (operation != "workflow.step.input"
                || resources.execution.status == StepExecutionStatus::WaitingInput)
                && arguments["expected_step_revision"] == resources.step.revision
                && arguments["control_version"] == root.instance.control_version,
            "待决定的原执行或控制版本已变化",
        )?;
        if operation == "workflow.step.input" {
            text(arguments, "input")?;
            require(
                !resources
                    .execution
                    .failure
                    .as_ref()
                    .is_some_and(|failure| failure.code == "platform_material_needed"),
                "已存在的平台材料应由经理原样转交",
            )?;
        }
        commit.witness(resources.reads.versions().iter().cloned())
    }
    fn close_execution_human_wait(
        &self,
        commit: &mut WorkflowCommit<'_>,
        resources: &WorkflowResources,
    ) -> Result<()> {
        let mut next = resources.execution.clone();
        if let Some(budget) = next.time_budget.as_mut() {
            budget.human_wait_started_at = None;
            budget.human_wait_closed = true;
            commit.step(resources, &mut next)?;
        }
        Ok(())
    }
    pub(crate) fn prepare_controller_decision(
        &self,
        commit: &mut WorkflowCommit<'_>,
        selected: &ResourceSnapshot,
        pending: ConfigurationRecord,
        approved: bool,
        reason: &str,
    ) -> Result<Value> {
        require(
            matches!(commit.context.actor(), WorkflowActor::Human),
            "决定需要实际 Human 来源",
        )?;
        let root = WorkflowHeaderFacts::read(self, &pending.definition_id)?;
        root.active()?;
        require(
            pending.r#type == "workflow_pending_decision"
                && pending.data["status"] == "pending"
                && root.instance.active_plan_revision_id == pending.data["plan_revision_id"],
            "原决定或计划已经变化",
        )?;
        commit.witness([version(selected), version(&root.header)])?;
        let operation = pending.data["request"]["operation"]
            .as_str()
            .unwrap_or("")
            .to_owned();
        let arguments = pending.data["request"]["arguments"].clone();
        let args = &arguments;
        if args["execution_id"].is_string()
            && !matches!(
                operation.as_str(),
                "workflow.step.confirm" | "workflow.step.accept" | "workflow.step.approve"
            )
        {
            self.require_step_decision(commit, &root, &operation, args)?;
        }
        if matches!(
            operation.as_str(),
            "workflow.step.confirm" | "workflow.step.accept" | "workflow.step.approve"
        ) {
            let resources = root.step(self, &text(args, "execution_id")?)?;
            self.require_execution_time(&resources)?;
            require(
                matches!(
                    resources.execution.status,
                    StepExecutionStatus::AwaitingAcceptance
                        | StepExecutionStatus::WaitingConfirmation
                ) && args["input_digest"] == resources.execution.input_digest
                    && args["outputs"] == json!(resources.execution.outputs),
                "待决定的执行与产物已变化",
            )?;
            commit.witness(resources.reads.versions().iter().cloned())?;
            if operation == "workflow.step.approve" {
                require(
                    pending.data["request"]["approval_basis"]
                        == self.native_approval_basis(commit, &resources)?,
                    "人工例外的评审、候选或输入已经变化",
                )?;
            }
            if approved {
                let definition = resources
                    .instance
                    .plan
                    .document
                    .steps
                    .iter()
                    .find(|s| s.key == resources.execution.step)
                    .ok_or_else(|| err("storage", "原决定步骤缺失"))?;
                if matches!(
                    operation.as_str(),
                    "workflow.step.confirm" | "workflow.step.approve"
                ) || definition.confirmation == ConfirmationPolicy::Human
                {
                    let mut selection = FactSelection::default();
                    selection.rule(&definition.completion);
                    let facts = resources.selected_facts(self.store.as_ref(), &selection)?;
                    commit.witness(facts.reads.versions().iter().cloned())?;
                    let mut payload = args.clone();
                    payload["reason"] = json!(reason);
                    payload["request_id"] = json!(pending.id);
                    payload["approval_basis"] = pending.data["request"]["approval_basis"].clone();
                    payload["exception_scope"] =
                        pending.data["request"]["interaction"]["issues"].clone();
                    let driver = one(
                        self.store.as_ref(),
                        "workflow_driver_root",
                        &root.instance.root_instance_id,
                    )?;
                    commit.witness([version(&driver)])?;
                    // A manager's request grants permission to sign these exact
                    // outputs. It never delegates business acceptance to Human.
                    payload["accept_result"] = json!(
                        operation != "workflow.step.approve"
                            && pending.data["actor"]["kind"] == "human"
                            && driver
                                .value
                                .as_ref()
                                .is_none_or(|row| row["data"]["mode"] != "agent")
                    );
                    self.prepare_native_confirmation(
                        commit,
                        &resources,
                        &payload,
                        operation == "workflow.step.approve",
                        &facts.facts,
                    )?;
                }
            } else if resources.execution.time_budget.is_some() {
                self.close_execution_human_wait(commit, &resources)?;
            }
        }
        let id=commit.new_record(record("workflow_human_decision",pending.id.clone(),pending.scope.clone(),reason.into(),1,None,json!({"approved":approved,"actor":commit.context.actor(),"plan_revision_id":pending.data["plan_revision_id"],"request":pending.data["request"],"reason":reason}),true),"workflow-human-decision")?;
        let result = json!({"id":pending.id,"status":if approved{"approved"}else{"rejected"},"decision_ref":id});
        let mut data = pending.data.clone();
        data["status"] = json!(if approved { "approved" } else { "rejected" });
        data["decision_ref"] = json!(id);
        let pending_id = pending.id.clone();
        commit.replace_record(selected, pending, data)?;
        if approved
            && matches!(
                operation.as_str(),
                "workflow.step.confirm" | "workflow.step.accept" | "workflow.step.approve"
            )
        {
            let resources = root.step(self, &text(args, "execution_id")?)?;
            self.settle_matching_step_decisions(commit, &resources, reason, Some(&pending_id))?;
        }
        Ok(result)
    }
    pub(crate) fn settle_matching_step_decisions(
        &self,
        commit: &mut WorkflowCommit<'_>,
        resources: &WorkflowResources,
        reason: &str,
        skip: Option<&str>,
    ) -> Result<()> {
        let selected = one(
            self.store.as_ref(),
            "workflow_step_decisions",
            &resources.execution.id,
        )?;
        commit.witness([version(&selected)])?;
        for raw in selected
            .value
            .as_ref()
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let row = ConfigurationRecord::from_storage(raw.clone())?;
            if skip == Some(row.id.as_str()) {
                continue;
            }
            let args = &row.data["request"]["arguments"];
            if row.definition_id != resources.record.id
                || row.data["plan_revision_id"] != resources.instance.active_plan_revision_id
                || args["input_digest"] != resources.execution.input_digest
                || args["outputs"] != json!(resources.execution.outputs)
            {
                continue;
            }
            let selected = ResourceSnapshot {
                resource: ResourceKey::new("governance", &row.id),
                revision: row.revision,
                value: Some(raw.clone()),
            };
            let id=commit.new_record(record("workflow_human_decision",row.id.clone(),row.scope.clone(),reason.into(),1,None,json!({"approved":true,"actor":commit.context.actor(),"plan_revision_id":row.data["plan_revision_id"],"request":row.data["request"],"reason":reason,"source":"workflow_confirmation"}),true),&format!("workflow-human-decision:{}",row.id))?;
            let mut data = row.data.clone();
            data["status"] = json!("approved");
            data["decision_ref"] = json!(id);
            commit.replace_record(&selected, row, data)?;
        }
        Ok(())
    }
}
