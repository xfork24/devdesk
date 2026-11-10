use crate::governance::{record, update_record};
use crate::*;
use std::collections::BTreeMap;

fn phase(step: &WorkflowStep) -> &str {
    step.parameters
        .get("phase")
        .and_then(Value::as_str)
        .unwrap_or("")
}
fn output_kind(p: &ProposalContent) -> &str {
    match p {
        ProposalContent::Requirement { .. } => "requirement",
        ProposalContent::Design { .. } => "design",
        ProposalContent::TaskPlan { .. } => "task_plan",
        ProposalContent::TestPlan { .. } => "test_plan",
        _ => "feature",
    }
}
fn current_document<'a>(
    s: &'a EngineeringState,
    feature: &str,
    task: Option<&str>,
    kind: &str,
) -> Option<&'a Document> {
    s.documents.iter().rev().find(|d| {
        s.document_in_feature(d, feature)
            && d.task_id.as_deref() == task
            && d.kind == kind
            && (kind == "requirement"
                || d.requirement_id.as_ref().is_none_or(|id| {
                    !s.documents
                        .iter()
                        .any(|new| new.supersedes.as_ref() == Some(id))
                }))
    })
}
fn owner_revision(s: &EngineeringState, owner: &str) -> Option<u64> {
    revision(s, owner)
        .or_else(|| {
            s.sessions
                .iter()
                .find(|x| x.id == owner)
                .map(|x| x.revision)
        })
        .or_else(|| {
            s.proposals
                .iter()
                .find(|x| x.id == owner)
                .map(|x| x.revision)
        })
}
fn transition_step(state: &mut WorkflowStepRuntime, status: &str, reason: &str) {
    if state.status != status || state.reason != reason {
        state.status = status.into();
        state.reason = reason.into();
        state.revision += 1;
    }
}
fn dependencies_output(
    runtime: &WorkflowRuntime,
    step: &WorkflowStep,
    key: &str,
) -> Option<String> {
    let input = dependency_output_source(runtime, step, key)?;
    runtime
        .steps
        .iter()
        .find(|step| step.key == input.step)
        .and_then(|step| step.outputs.get(&input.port))
        .cloned()
}
fn dependency_output_source(
    runtime: &WorkflowRuntime,
    step: &WorkflowStep,
    key: &str,
) -> Option<WorkflowPortReference> {
    if let Some(input) = step.inputs.get(key) {
        return Some(input.clone());
    }
    step.dependencies
        .iter()
        .rev()
        .filter_map(|key| runtime.steps.iter().find(|s| &s.key == key))
        .find(|s| s.outputs.contains_key(key))
        .map(|s| WorkflowPortReference {
            step: s.key.clone(),
            port: key.into(),
        })
}

// Discussion outputs remain live until their adoption step is finished. Follow
// exact ports through compiled calls; unrelated sessions and consumed inputs stay frozen.
fn pending_proposal_outputs(
    runtime: &WorkflowRuntime,
    plan: &WorkflowDocument,
) -> BTreeMap<String, Vec<String>> {
    let mut outputs = BTreeMap::<String, Vec<String>>::new();
    for adopt in plan
        .steps
        .iter()
        .filter(|step| step.kind == "proposal.adopt")
    {
        if !runtime.steps.iter().any(|state| {
            state.key == adopt.key
                && !["completed", "skipped", "failed"].contains(&state.status.as_str())
        }) {
            continue;
        }
        let mut source = dependency_output_source(runtime, adopt, "proposal");
        let mut path = vec![(adopt.key.clone(), "proposal".into())];
        while let Some(input) = source.take() {
            if path.iter().any(|(key, _)| key == &input.step) {
                break;
            }
            let Some(step) = plan.steps.iter().find(|step| step.key == input.step) else {
                break;
            };
            path.push((input.step, input.port.clone()));
            if step.kind == "agent.interact" && input.port == "proposal" {
                for (key, port) in path {
                    outputs.entry(key).or_default().push(port);
                }
                break;
            }
            if step.kind == "workflow.call" {
                source = step
                    .parameters
                    .get("call_outputs")
                    .and_then(|ports| ports.get(&input.port))
                    .and_then(|value| serde_json::from_value(value.clone()).ok());
            }
        }
    }
    outputs
}

fn discussion_session(s: &EngineeringState, initial: &str) -> String {
    let agent_instance = |id: &str| {
        s.governance
            .iter()
            .find(|r| r.kind == "agent_session" && r.definition_id == id)
            .and_then(|r| r.data["agent_instance_id"].as_str())
    };
    let mut current = initial;
    for _ in 0..s.sessions.len() {
        let Some(previous) = s.sessions.iter().find(|session| session.id == current) else {
            break;
        };
        let Some(next) = s.sessions.iter().rev().find(|session| {
            session.predecessor.as_deref() == Some(current)
                && session.owner_id == previous.owner_id
                && session.agent == previous.agent
                && agent_instance(&session.id) == agent_instance(&previous.id)
        }) else {
            break;
        };
        current = &next.id;
    }
    current.into()
}
fn accepted_condition(
    s: &EngineeringState,
    runtime: &WorkflowRuntime,
    c: &WorkflowCondition,
) -> bool {
    match c {
        WorkflowCondition::Always => true,
        WorkflowCondition::All { conditions } => {
            conditions.iter().all(|c| accepted_condition(s, runtime, c))
        }
        WorkflowCondition::Any { conditions } => {
            conditions.iter().any(|c| accepted_condition(s, runtime, c))
        }
        WorkflowCondition::OutputPresent { step } => runtime
            .steps
            .iter()
            .any(|r| r.key == *step && !r.outputs.is_empty()),
        WorkflowCondition::Accepted { kind } => s.deliveries.iter().any(|d| {
            d.feature_id == runtime.feature_id
                && d.task_id == runtime.task_id
                && d.kind == *kind
                && s.accepted(d)
        }),
    }
}

impl Engine {
    pub fn validate_workflow_command(&self, method: &str, c: &Command) -> Result<()> {
        self.enforce_workflow_action(&self.store.snapshot()?, method, c)
    }

    pub(crate) fn enforce_workflow_action(
        &self,
        s: &EngineeringState,
        method: &str,
        c: &Command,
    ) -> Result<()> {
        let target = c.target.as_deref().unwrap_or("");
        if method == "proposal.adopt"
            && s.proposals
                .iter()
                .find(|p| p.id == target)
                .is_some_and(|p| crate::agent_tasks::formal_session(s, &p.session_id))
        {
            return Err(err(
                "blocked",
                "正式执行会话的回复不能再次采纳为需求提案；请在执行与交付的当前工作流查看执行状态，模型报告完成后执行人类 Approve；中断时使用技术重试",
            ));
        }
        let owner = if let Some(d) = s.deliveries.iter().find(|d| d.id == target) {
            d.task_id.as_deref().unwrap_or(&d.feature_id)
        } else if let Some(m) = s.merges.iter().find(|m| m.id == target) {
            m.task_id.as_deref().unwrap_or(&m.feature_id)
        } else if let Some(a) = s.attempts.iter().find(|a| a.id == target) {
            a.task_id.as_deref().unwrap_or(&a.feature_id)
        } else if let Some(session) = s.sessions.iter().find(|r| r.id == target) {
            &session.owner_id
        } else if let Some(proposal) = s.proposals.iter().find(|r| r.id == target) {
            proposal.target.as_deref().unwrap_or("")
        } else {
            target
        };
        let feature = s
            .tasks
            .iter()
            .find(|t| t.id == owner)
            .map_or(owner, |t| t.feature_id.as_str());
        let Some(lock) = s.feature_configuration(feature) else {
            require(
                !s.features.iter().any(|r| r.id == feature)
                    || ![
                        "attempt.start",
                        "merge.request",
                        "review.request",
                        "session.open",
                        "task.create",
                    ]
                    .contains(&method),
                "Feature 必须冻结当前版本化配置后才能执行",
            )?;
            return Ok(());
        };
        if method == "session.open" {
            require(
                lock.agent_refs
                    .values()
                    .any(|agent| c.payload["agent"].as_str() == Some(agent.as_str())),
                "Session 必须使用 Feature 锁定的 Agent 组合",
            )?;
        }
        let action = match method {
            "attempt.start" => "attempt.execute",
            "delivery.submit" => "attempt.execute",
            "review.request" | "review.submit" | "override.accept" => "review",
            "merge.request" => "merge.prepare",
            "merge.apply" => "merge.apply",
            "proposal.adopt" => "proposal.adopt",
            _ => return Ok(()),
        };
        require(!s.workflow_paused(owner), "工作流已暂停")?;
        let runtime: WorkflowRuntime = s
            .governance
            .iter()
            .rev()
            .find(|r| {
                r.kind == "workflow_instance"
                    && r.definition_id == owner
                    && r.data["status"] != "cancelled"
            })
            .ok_or_else(|| err("workflow_blocked", "对象流程尚未建立"))?
            .decode()?;
        let plan = if runtime.task_id.is_some() {
            &lock.task_plan
        } else {
            &lock.plan
        };
        let active = plan.steps.iter().find(|step| {
            let runtime_step = runtime.steps.iter().find(|r| r.key == step.key);
            let correct_target = if method == "merge.apply" {
                dependencies_output(&runtime, step, "merge").as_deref() == Some(target)
            } else if action == "review" {
                runtime_step
                    .and_then(|s| s.outputs.get("delivery"))
                    .or_else(|| None)
                    .is_none_or(|id| id == target)
                    && dependencies_output(&runtime, step, "delivery").is_none_or(|id| id == target)
            } else if method == "delivery.submit" {
                runtime_step
                    .and_then(|s| s.outputs.get("attempt"))
                    .is_some_and(|id| id == target)
            } else {
                true
            };
            correct_target
                && (step.kind == action
                    || (action == "review"
                        && ["delivery.review", "merge.review"].contains(&step.kind.as_str())))
                && (method != "attempt.start"
                    || phase(step)
                        == c.payload["phase"]
                            .as_str()
                            .unwrap_or(if runtime.task_id.is_some() {
                                "implementation"
                            } else {
                                ""
                            }))
                && runtime.steps.iter().any(|r| {
                    r.key == step.key
                        && !["completed", "skipped", "failed"].contains(&r.status.as_str())
                        && c.payload["workflow_step_instance_id"]
                            .as_str()
                            .is_none_or(|id| r.id == id)
                })
                && step.dependencies.iter().all(|key| {
                    runtime.steps.iter().any(|r| {
                        r.key == *key && ["completed", "skipped"].contains(&r.status.as_str())
                    })
                })
        });
        require(
            active.is_some(),
            "当前工作流步骤不允许该动作，或前置交付/人工确认尚未完成",
        )
    }

    pub(crate) fn workflow_control(
        &self,
        method: &str,
        c: &Command,
        s: &mut EngineeringState,
    ) -> Result<Value> {
        let target = c
            .target
            .as_deref()
            .ok_or_else(|| err("invalid_params", "需要流程或步骤目标"))?;
        let found = s
            .governance
            .iter()
            .find(|r| {
                r.kind == "workflow_instance"
                    && (r.id == target
                        || r.data["steps"]
                            .as_array()
                            .is_some_and(|steps| steps.iter().any(|x| x["id"] == target)))
            })
            .cloned()
            .ok_or_else(|| err("not_found", "流程实例不存在"))?;
        let mut runtime: WorkflowRuntime = found.decode()?;
        require(
            !["completed", "cancelled"].contains(&runtime.status.as_str()),
            "流程已结束并冻结；需要新治理或新工作时创建后继 Feature",
        )?;
        let owned_step_ids: Vec<_> = runtime.steps.iter().map(|step| step.id.clone()).collect();
        if method == "workflow.pause" || method == "workflow.resume" {
            require(
                c.expected_revision == Some(found.revision),
                "流程控制版本冲突",
            )?;
            runtime.paused = method == "workflow.pause";
            runtime.dirty = true;
            update_record(
                s.governance.iter_mut().find(|r| r.id == found.id).unwrap(),
                json!(runtime),
            );
            return Ok(json!({"id":found.id}));
        }
        let index = runtime
            .steps
            .iter()
            .position(|r| r.id == target)
            .ok_or_else(|| err("not_found", "步骤不存在"))?;
        let old = runtime.steps[index].clone();
        require(
            c.expected_revision == Some(old.revision),
            "步骤已改变，请刷新",
        )?;
        if method == "workflow.step.confirm" {
            require(old.status == "waiting_confirmation", "当前步骤不等待确认")?;
            require(
                c.payload["input_hash"] == old.input_hash,
                "确认的版本已变化",
            )?;
            let reports = crate::model_approvals::reports(s, &old);
            if reports.is_empty() {
                if let Some(did) = old.outputs.get("delivery") {
                    let d = s.delivery(did)?;
                    require(s.accepted(d), "交付证据已失效")?;
                    if d.merge_id.is_some() {
                        self.check_merge_target(s, d)?;
                    }
                }
            } else {
                require(
                    crate::model_approvals::input_hash(s, &runtime.configuration_lock_id, &old)
                        == old.input_hash,
                    "模型结论已变化，请刷新后重新 Approve",
                )?;
            }
            let confirmation = json!({"input_hash":old.input_hash,"step_id":old.id,"round":old.round,"outputs":old.outputs,"actor":"human:local","confirmed_at":now(),"human_approve":true,"model_approvals":reports,"overwrite_model":c.payload["overwrite_model"]==true,"reason":c.payload["reason"]});
            s.governance.push(record(
                "workflow_confirmation",
                old.id.clone(),
                found.scope.clone(),
                "人工推进决定".into(),
                old.round,
                None,
                confirmation.clone(),
                true,
            ));
            if !reports.is_empty() {
                let lock: FeatureConfiguration =
                    s.configuration(&runtime.configuration_lock_id)?.decode()?;
                let plan = if runtime.task_id.is_some() {
                    &lock.task_plan
                } else {
                    &lock.plan
                };
                let definition = plan.steps.iter().find(|step| step.key == old.key).unwrap();
                self.validate_human_approval(s, definition, &old, &reports, c)?;
            }
            runtime.steps[index].confirmation = Some(confirmation);
            runtime.steps[index].status = "completed".into();
            runtime.steps[index].revision += 1;
        } else if method == "workflow.step.retry" {
            require(
                ["failed", "blocked"].contains(&old.status.as_str()),
                "当前步骤无需重试",
            )?;
            let lock: FeatureConfiguration =
                s.configuration(&runtime.configuration_lock_id)?.decode()?;
            let plan = if runtime.task_id.is_some() {
                &lock.task_plan
            } else {
                &lock.plan
            };
            let limit = plan
                .steps
                .iter()
                .find(|step| step.key == old.key)
                .unwrap()
                .retry_limit;
            let retries = s
                .governance
                .iter()
                .filter(|r| r.kind == "workflow_retry" && r.definition_id == old.id)
                .count();
            require(
                retries < (limit as usize),
                "技术重试次数已到上限，请检查原因后业务返工",
            )?;
            s.governance.push(record(
                "workflow_retry",
                old.id.clone(),
                found.scope.clone(),
                "技术重试".into(),
                retries as u64 + 1,
                None,
                json!({"step":old.id,"time":now()}),
                true,
            ));
            let failed_intents: Vec<_> = s
                .governance
                .iter()
                .filter(|r| {
                    r.kind == "workflow_intent"
                        && r.definition_id == old.id
                        && r.data["status"] == "failed"
                })
                .map(|r| r.id.clone())
                .collect();
            for id in failed_intents {
                let r = s.governance.iter_mut().find(|r| r.id == id).unwrap();
                let mut data = r.data.clone();
                data["status"] = json!("prepared");
                data["error"] = Value::Null;
                update_record(r, data);
            }
            let definition = plan.steps.iter().find(|step| step.key == old.key).unwrap();
            if definition.kind == "merge.apply" {
                let mid = old
                    .outputs
                    .get("merge")
                    .ok_or_else(|| err("blocked", "缺少候选"))?;
                let m = s.merge(mid)?;
                require(
                    s.advances
                        .iter()
                        .any(|a| a.id == *mid && a.status == AdvanceStatus::NotApplied),
                    "必须先证明接纳未执行，或从候选准备返工",
                )?;
                let delivery = s
                    .deliveries
                    .iter()
                    .rev()
                    .find(|d| d.merge_id.as_ref() == Some(mid))
                    .ok_or_else(|| err("blocked", "候选交付缺失"))?;
                self.check_merge_target(s, delivery)?;
                require(s.accepted(delivery), "候选证据失效，请从候选准备返工")?;
                let _ = m;
                self.workflow_intent(s, &old, "merge.apply", mid, json!({}))?;
            }
            if let Some(did) = old.outputs.get("delivery") {
                if ["delivery.review", "merge.review"].contains(&definition.kind.as_str()) {
                    require(
                        !s.reviews.iter().any(|r| {
                            r.delivery_id == *did
                                && [Decision::ChangesRequested, Decision::Pass]
                                    .contains(&r.decision)
                        }),
                        "已有质量决定，需从执行或候选准备业务返工",
                    )?;
                    let node = format!("review:{did}");
                    if let Some(run) = s.runs.iter().rev().find(|r| {
                        r.node == node
                            && [Status::Failed, Status::Interrupted, Status::Cancelled]
                                .contains(&r.status)
                    }) {
                        let run = run.id.clone();
                        self.workflow_intent(s, &old, "run.retry", &run, json!({}))?;
                    }
                }
            }
            if let Some(aid) = old.outputs.get("attempt") {
                let runs: Vec<_> = s
                    .latest_runs(Some(aid), None)
                    .into_iter()
                    .filter(|r| {
                        [Status::Failed, Status::Interrupted, Status::Cancelled].contains(&r.status)
                    })
                    .map(|r| r.id.clone())
                    .collect();
                for run in runs {
                    self.workflow_intent(s, &old, "run.retry", &run, json!({}))?;
                }
            }
            runtime.steps[index].status = "active".into();
            runtime.steps[index].reason.clear();
            runtime.steps[index].revision += 1;
        } else {
            let lock: FeatureConfiguration =
                s.configuration(&runtime.configuration_lock_id)?.decode()?;
            let plan = if runtime.task_id.is_some() {
                &lock.task_plan
            } else {
                &lock.plan
            };
            let source = c.payload["from_step"].as_str().unwrap_or(old.key.as_str());
            let source = plan
                .steps
                .iter()
                .find(|step| step.key == source && step.kind == "workflow.call")
                .and_then(|step| step.parameters.get("call_entry"))
                .and_then(Value::as_str)
                .unwrap_or(source);
            require(
                plan.steps.iter().any(|step| step.key == source),
                "未知返工入口",
            )?;
            let mut affected = std::collections::BTreeSet::from([source.to_string()]);
            loop {
                let size = affected.len();
                for step in &plan.steps {
                    if step.dependencies.iter().any(|key| affected.contains(key)) {
                        affected.insert(step.key.clone());
                    }
                }
                if size == affected.len() {
                    break;
                }
            }
            for step in runtime
                .steps
                .iter_mut()
                .filter(|step| affected.contains(&step.key))
            {
                s.governance.push(record(
                    "workflow_step_history",
                    step.id.clone(),
                    found.scope.clone(),
                    "返工前步骤".into(),
                    step.round,
                    None,
                    json!(step),
                    true,
                ));
                if let Some(aid) = step.outputs.get("attempt") {
                    if let Some(a) = s.attempts.iter_mut().find(|a| &a.id == aid) {
                        if [Status::Running, Status::WaitingReview].contains(&a.status) {
                            a.status = Status::Interrupted;
                            a.paused = true;
                            a.revision += 1;
                        }
                    }
                    for run in s
                        .runs
                        .iter_mut()
                        .filter(|r| r.attempt_id.as_ref() == Some(aid))
                    {
                        if run.status == Status::Running {
                            run.cancel_requested = true;
                        } else if run.status == Status::Queued {
                            run.status = Status::Cancelled;
                        }
                    }
                }
                *step = WorkflowStepRuntime {
                    id: id(),
                    key: step.key.clone(),
                    round: step.round + 1,
                    status: "pending".into(),
                    input_hash: String::new(),
                    outputs: BTreeMap::new(),
                    reason: String::new(),
                    revision: 1,
                    confirmation: None,
                };
            }
            if affected.iter().any(|k| {
                plan.steps
                    .iter()
                    .any(|s| s.key == *k && s.kind == "tasks.foreach")
            }) {
                for child in s
                    .governance
                    .iter_mut()
                    .filter(|r| r.kind == "workflow_instance" && r.data["parent_id"] == found.id)
                {
                    let mut data = child.data.clone();
                    data["paused"] = json!(true);
                    data["status"] = json!("cancelled");
                    update_record(child, data);
                }
                for attempt in s.attempts.iter_mut().filter(|a| {
                    a.task_id
                        .as_ref()
                        .is_some_and(|tid| runtime.task_members.contains(tid))
                }) {
                    attempt.paused = true;
                    if [Status::Running, Status::WaitingReview].contains(&attempt.status) {
                        attempt.status = Status::Interrupted;
                        attempt.revision += 1;
                    }
                }
                let cancelled: Vec<_> = s
                    .attempts
                    .iter()
                    .filter(|a| {
                        a.task_id
                            .as_ref()
                            .is_some_and(|tid| runtime.task_members.contains(tid))
                    })
                    .map(|a| a.id.clone())
                    .collect();
                for run in s.runs.iter_mut().filter(|r| {
                    r.attempt_id
                        .as_ref()
                        .is_some_and(|id| cancelled.contains(id))
                }) {
                    if run.status == Status::Running {
                        run.cancel_requested = true;
                    } else if run.status == Status::Queued {
                        run.status = Status::Cancelled;
                    }
                }
                runtime.task_members.clear();
                runtime.task_plan_revision = None;
                runtime.generation += 1;
            }
        }
        let valid: Vec<_> = runtime.steps.iter().map(|step| step.id.as_str()).collect();
        for intent in s.governance.iter_mut().filter(|r| {
            r.kind == "workflow_intent"
                && r.data["status"] == "prepared"
                && owned_step_ids.contains(&r.definition_id)
                && !valid.contains(&r.definition_id.as_str())
        }) {
            let mut data = intent.data.clone();
            data["status"] = json!("superseded");
            update_record(intent, data);
        }
        runtime.dirty = true;
        runtime.status = "active".into();
        update_record(
            s.governance.iter_mut().find(|r| r.id == found.id).unwrap(),
            json!(runtime),
        );
        Ok(json!({"id":found.id}))
    }

    pub fn evaluate_workflows(&self) -> Result<()> {
        let snapshot = self.store.snapshot()?;
        if !snapshot.governance.iter().any(|r| {
            r.kind == "workflow_instance"
                && r.data["status"] != "completed"
                && r.data["paused"] != true
        }) {
            return Ok(());
        }
        self.store.transact(None, &mut |s| {
            let instances: Vec<_> = s
                .governance
                .iter()
                .filter(|r| r.kind == "workflow_instance")
                .cloned()
                .collect();
            for record_instance in instances {
                let mut runtime: WorkflowRuntime = record_instance.decode()?;
                if runtime.paused || runtime.status == "completed" || runtime.status == "cancelled"
                {
                    continue;
                }
                let lock: FeatureConfiguration =
                    s.configuration(&runtime.configuration_lock_id)?.decode()?;
                require(lock.compiler_version == 3, "运行器只支持 Workflow compiler v3")?;
                let plan = if runtime.task_id.is_some() {
                    &lock.task_plan
                } else {
                    &lock.plan
                };
                let live_proposals = pending_proposal_outputs(&runtime, plan);
                for step in &plan.steps {
                    let Some(index) = runtime.steps.iter().position(|r| r.key == step.key) else {
                        return Err(err("invalid_workflow", "运行步骤与冻结计划不匹配"));
                    };
                    if runtime.steps[index].status == "waiting_confirmation" {
                        if let Some(did) = runtime.steps[index].outputs.get("delivery") {
                            if s.delivery(did).is_ok_and(|d| {
                                d.stale || !s.delivery_inputs_current(d)
                                    || (crate::model_approvals::reports(s,&runtime.steps[index]).is_empty() && !s.accepted(d))
                                    || self.check_merge_target(s, d).is_err()
                            }) {
                                runtime.steps[index].status = "blocked".into();
                                runtime.steps[index].reason =
                                    "确认依据的候选或证据已失效，请从上游重新构建".into();
                                runtime.steps[index].confirmation = None;
                                runtime.steps[index].revision += 1;
                                continue;
                            }
                        }
                    }
                    let refresh_ports = live_proposals.get(&step.key);
                    let refresh_model_approval = runtime.steps[index].status == "waiting_confirmation"
                        && !crate::model_approvals::reports(s, &runtime.steps[index]).is_empty();
                    if ["skipped", "failed"].contains(&runtime.steps[index].status.as_str())
                        || (refresh_ports.is_none()
                            && (runtime.steps[index].status == "completed"
                                || (runtime.steps[index].status == "waiting_confirmation" && !refresh_model_approval)))
                    {
                        continue;
                    }
                    if s.workflow_paused(&runtime.owner_id) {
                        continue;
                    }
                    let skipped_guard=step.parameters.get("call_guards").and_then(Value::as_array).is_some_and(|guards|guards.iter().any(|key|runtime.steps.iter().any(|r|Some(r.key.as_str())==key.as_str()&&r.status=="skipped")));
                    if skipped_guard {transition_step(&mut runtime.steps[index],"skipped","父调用未进入");continue;}
                    if step.kind=="workflow.call" {
                        let blocked=runtime.steps.iter().find(|r|step.dependencies.contains(&r.key)&&r.status=="blocked").map(|r|format!("子流程 {} 阻塞：{}",r.key,r.reason));
                        if let Some(reason)=blocked {transition_step(&mut runtime.steps[index],"blocked",&reason);continue;}
                        if step.dependencies.iter().any(|key|runtime.steps.iter().any(|r|&r.key==key&&r.status=="failed")){transition_step(&mut runtime.steps[index],"failed","子流程失败，请显式返工");continue;}
                    }
                    if !step.dependencies.iter().all(|key| {
                        runtime.steps.iter().any(|r| {
                            r.key == *key && ["completed", "skipped"].contains(&r.status.as_str())
                        })
                    }) {
                        if let Some(ports) = refresh_ports {
                            let state = &mut runtime.steps[index];
                            let before = json!(state);
                            for port in ports {
                                state.outputs.remove(port);
                                for (name, output) in &step.delivery_outputs {
                                    if &output.source_port == port { state.outputs.remove(name); }
                                }
                            }
                            state.confirmation = None;
                            state.input_hash.clear();
                            state.status = "waiting_dependencies".into();
                            state.reason = "等待上游讨论或确认完成".into();
                            if before != json!(state) {
                                state.revision += 1;
                            }
                        }
                        continue;
                    }
                    if step.kind != "join"
                        && step.dependencies.iter().any(|key| {
                            runtime
                                .steps
                                .iter()
                                .any(|r| r.key == *key && r.status == "skipped")
                        })
                    {
                        transition_step(&mut runtime.steps[index],"skipped","上游分支未进入");
                        continue;
                    }
                    if !accepted_condition(s, &runtime, &step.condition) {
                        if step.kind == "branch" {
                            transition_step(&mut runtime.steps[index],"skipped","分支条件不满足");
                        } else {
                            transition_step(&mut runtime.steps[index],"blocked","等待流程条件满足");
                        }
                        continue;
                    }
                    let mut state = runtime.steps[index].clone();
                    let before = json!(state);
                    for (name, output) in &step.delivery_outputs {
                        if name != &output.source_port { state.outputs.remove(name); }
                    }
                    state.status = "active".into();
                    state.reason.clear();
                    let effect = s
                        .governance
                        .iter()
                        .find(|r| {
                            r.kind == "workflow_intent"
                                && r.definition_id == state.id
                                && r.data["status"] == "failed"
                                && r.data["method"] != "proposal.extract"
                        })
                        .cloned();
                    if let Some(failed) = effect {
                        state.status = "blocked".into();
                        state.reason = failed.data["error"].as_str().unwrap_or("操作未完成").into();
                        if before!=json!(state){state.revision+=1;}
                        runtime.steps[index] = state;
                        continue;
                    }
                    let complete = self.evaluate_step(
                        s,
                        &record_instance.id,
                        &mut runtime,
                        step,
                        &mut state,
                        &lock,
                    )?;
                    if complete {
                        let missing = step.delivery_outputs.iter().find(|(_, declaration)| !state.outputs.contains_key(&declaration.source_port));
                        if let Some((name, declaration)) = missing {
                            state.status = "blocked".into();
                            state.reason = format!("交付物 {name} 尚未产生：缺少 {} 输出", declaration.source_port);
                            if before != json!(state) { state.revision += 1; }
                            runtime.steps[index] = state;
                            continue;
                        }
                        for (name, declaration) in &step.delivery_outputs {
                            state.outputs.insert(name.clone(), state.outputs[&declaration.source_port].clone());
                        }
                        let reports = crate::model_approvals::reports(s, &state);
                        let input_hash = crate::model_approvals::input_hash(s, &runtime.configuration_lock_id, &state);
                        if state.input_hash != input_hash {
                            state.confirmation = None;
                        }
                        state.input_hash = input_hash;
                        if (step.advance_mode == AdvanceMode::HumanConfirmation || !reports.is_empty())
                            && state
                                .confirmation
                                .as_ref()
                                .is_none_or(|v| v["input_hash"] != state.input_hash)
                        {
                            state.status = "waiting_confirmation".into();
                            state.reason = if reports.iter().any(|report| report["approve"] == false) {
                                "模型未 Approve：可返工，或填写理由由人类覆盖结论后 Approve"
                            } else if !reports.is_empty() {
                                "模型已 Approve，等待人类 Approve 后继续"
                            } else { "结果已齐备，等待人确认后推进" }.into();
                        } else {
                            state.status = "completed".into();
                        }
                    }
                    if before != json!(state) {
                        state.revision += 1;
                    }
                    runtime.steps[index] = state;
                }
                let engineering_complete = runtime.task_id.as_ref().map_or_else(
                    || {
                        s.feature(&runtime.feature_id)
                            .is_ok_and(|f| f.status == Status::Completed)
                    },
                    |id| s.task(id).is_ok_and(|t| t.status == Status::Completed),
                );
                runtime.status = if runtime
                    .steps
                    .iter()
                    .all(|s| ["completed", "skipped"].contains(&s.status.as_str()))
                {
                    if engineering_complete {
                        "completed"
                    } else {
                        "blocked"
                    }
                } else if runtime
                    .steps
                    .iter()
                    .any(|s| s.status == "waiting_confirmation")
                {
                    "waiting_confirmation"
                } else if runtime.steps.iter().any(|s| s.status == "waiting_input") {
                    "waiting_input"
                } else {
                    "active"
                }
                .into();
                runtime.dirty = false;
                if record_instance.data["steps"]!=json!(runtime.steps)||record_instance.data["status"]!=runtime.status {event(s,&record_instance.id,"workflow.changed",&json!({"owner":runtime.owner_id,"status":runtime.status,"steps":runtime.steps.iter().map(|step|json!({"id":step.id,"key":step.key,"round":step.round,"status":step.status})).collect::<Vec<_>>()} ).to_string());}
                let r = s
                    .governance
                    .iter_mut()
                    .find(|r| r.id == record_instance.id)
                    .unwrap();
                update_record(r, json!(runtime));
            }
            Ok(json!({"evaluated":true}))
        })?;
        Ok(())
    }

    fn workflow_intent(
        &self,
        s: &mut EngineeringState,
        step: &WorkflowStepRuntime,
        method: &str,
        target: &str,
        payload: Value,
    ) -> Result<()> {
        let mut action_input = json!({"target":target,"payload":payload});
        if method == "merge.apply" {
            let retries = s
                .governance
                .iter()
                .filter(|r| r.kind == "workflow_retry" && r.definition_id == step.id)
                .count();
            if retries > 0 {
                action_input["retry_epoch"] = json!(retries);
            }
        }
        let key = format!(
            "workflow:{}:{}:{}",
            step.id,
            method,
            configuration_hash(&action_input)
        );
        if s.governance
            .iter()
            .any(|r| r.kind == "workflow_intent" && r.data["action_key"] == key)
        {
            return Ok(());
        }
        let mut payload = payload;
        payload["workflow_step_instance_id"] = json!(step.id);
        let feature = s
            .governance
            .iter()
            .find(|r| {
                r.kind == "workflow_instance"
                    && r.data["steps"]
                        .as_array()
                        .is_some_and(|steps| steps.iter().any(|x| x["id"] == step.id))
            })
            .map(|r| r.scope.clone())
            .unwrap_or(Scope::Global);
        s.governance.push(record("workflow_intent",step.id.clone(),feature,method.into(),1,None,json!({"action_key":key,"status":"prepared","method":method,"command":{"command_id":key,"target":target,"expected_revision":owner_revision(s,target),"payload":payload}}),false));
        Ok(())
    }

    fn evaluate_step(
        &self,
        s: &mut EngineeringState,
        instance_id: &str,
        runtime: &mut WorkflowRuntime,
        step: &WorkflowStep,
        state: &mut WorkflowStepRuntime,
        lock: &FeatureConfiguration,
    ) -> Result<bool> {
        let owner = runtime.owner_id.clone();
        match step.kind.as_str() {
            "agent.interact" => {
                let wanted = step.interaction.output_kind.as_str();
                let session = s
                    .sessions
                    .iter()
                    .rev()
                    .find(|session| {
                        session.owner_id == owner
                            && crate::agent_tasks::session_workflow_context(s, &session.id)
                                .is_some_and(|context| {
                                    context["step_instance_id"] == state.id
                                        && context["recipe_node"].is_null()
                                })
                    })
                    .map(|session| session.id.clone())
                    .or_else(|| state.outputs.get("session").cloned());
                if let Some(session) = session {
                    let session = discussion_session(s, &session);
                    if state.outputs.get("session") != Some(&session)
                        || (state.round == 1 && !state.outputs.contains_key("turn"))
                        || state
                            .outputs
                            .get("turn")
                            .and_then(|id| s.turns.iter().find(|t| &t.id == id))
                            .is_some_and(|turn| turn.session_id != session)
                    {
                        if let Some(turn) = s.turns.iter().find(|turn| turn.session_id == session) {
                            state.outputs.insert("turn".into(), turn.id.clone());
                        } else {
                            state.outputs.remove("turn");
                        }
                    }
                    state.outputs.insert("session".into(), session.clone());
                    state.outputs.remove("proposal");
                    let first_turn = state
                        .outputs
                        .get("turn")
                        .and_then(|id| s.turns.iter().position(|t| &t.id == id));
                    if let Some(proposal) = s.proposals.iter().rev().find(|p| {
                        first_turn.is_some_and(|index| {
                            s.turns
                                .iter()
                                .position(|t| t.id == p.turn_id)
                                .is_some_and(|turn| turn >= index)
                        }) && p.session_id == session
                            && p.target.as_deref() == Some(owner.as_str())
                            && output_kind(&p.content) == wanted
                            && p.status != ProposalStatus::Rejected
                            && p.status != ProposalStatus::Stale
                            && !s
                                .proposals
                                .iter()
                                .any(|new| new.supersedes.as_ref() == Some(&p.id))
                            && p.dependencies.iter().all(|id| {
                                !s.documents.iter().any(|new| {
                                    new.supersedes.as_ref() == Some(id)
                                        && s.document_in_feature(new, &runtime.feature_id)
                                })
                            })
                    }) {
                        state.outputs.insert("proposal".into(), proposal.id.clone());
                        return Ok(true);
                    }
                    let turns: Vec<_> = s
                        .turns
                        .iter()
                        .filter(|t| t.session_id == session)
                        .cloned()
                        .collect();
                    if turns.len() >= step.interaction.max_turns as usize {
                        state.status = "blocked".into();
                        state.reason = "讨论已达轮数上限，请整理材料或返工".into();
                        return Ok(false);
                    }
                    if !state.outputs.contains_key("turn") {
                        let rev = s
                            .sessions
                            .iter()
                            .find(|s| s.id == session)
                            .map(|s| s.revision);
                        self.workflow_intent(s,state,"conversation.send",&session,json!({"client_message_id":format!("workflow:{}:initial",state.id),"input":json!({"workflow_step_instance_id":state.id}).to_string(),"expected_session_revision":rev}))?;
                    }
                    if let Some(turn) = turns
                        .iter()
                        .skip(
                            state
                                .outputs
                                .get("turn")
                                .and_then(|id| turns.iter().position(|t| &t.id == id))
                                .unwrap_or(turns.len()),
                        )
                        .rev()
                        .find(|t| t.status == TurnStatus::Completed)
                    {
                        if !s.proposals.iter().any(|p| p.turn_id == turn.id) {
                            self.workflow_intent(
                                s,
                                state,
                                "proposal.extract",
                                &session,
                                json!({"turn_id":turn.id,"target":owner}),
                            )?;
                        }
                    }
                    state.status = "waiting_input".into();
                    state.reason = s
                        .governance
                        .iter()
                        .rev()
                        .find(|r| {
                            r.kind == "workflow_intent"
                                && r.definition_id == state.id
                                && r.data["method"] == "proposal.extract"
                                && r.data["status"] == "failed"
                        })
                        .and_then(|r| r.data["error"].as_str())
                        .map(|error| format!("提案尚不可采纳：{error}；请继续讨论或编辑提案"))
                        .unwrap_or("继续角色讨论并形成结构化提案".into());
                } else {
                    if state.round == 1 && ["requirement", "design", "test_plan"].contains(&wanted)
                    {
                        let kind = if wanted == "test_plan" {
                            "test"
                        } else {
                            wanted
                        };
                        if let Some(doc) = current_document(
                            s,
                            &runtime.feature_id,
                            runtime.task_id.as_deref(),
                            kind,
                        ) {
                            state.outputs.insert("document".into(), doc.id.clone());
                            return Ok(true);
                        }
                    }
                    let slot = step
                        .participant_slot
                        .as_ref()
                        .and_then(|slot| lock.agent_refs.get(slot))
                        .ok_or_else(|| err("configuration_required", "讨论步骤未绑定 Agent"))?;
                    self.workflow_intent(s,state,"session.open",&owner,json!({"agent":slot,"work_type":if lock.plan.binding_slots.get(step.participant_slot.as_deref().unwrap_or("")).or_else(||lock.task_plan.binding_slots.get(step.participant_slot.as_deref().unwrap_or(""))).is_some_and(|slot|slot.role==AgentRole::Architect){"design"}else{"discussion"}}))?;
                }
                Ok(false)
            }
            "proposal.adopt" => {
                if let Some(proposal) = dependencies_output(runtime, step, "proposal") {
                    let p = s
                        .proposals
                        .iter()
                        .find(|p| p.id == proposal)
                        .ok_or_else(|| err("not_found", "步骤提案不存在"))?;
                    state.outputs.insert("proposal".into(), p.id.clone());
                    if p.status == ProposalStatus::Accepted {
                        if let Some(document) = p
                            .adopted_objects
                            .iter()
                            .find(|id| s.documents.iter().any(|d| &d.id == *id))
                        {
                            state.outputs.insert("document".into(), document.clone());
                        }
                        return Ok(true);
                    }
                    state.status = "waiting_input".into();
                    state.reason = match p.status {
                        ProposalStatus::Rejected => "提案已拒绝，请继续讨论或编辑新提案",
                        ProposalStatus::Stale => "提案已失效，请基于当前输入重新讨论",
                        _ => "等待人采纳提案",
                    }
                    .into();
                } else if let Some(doc) = dependencies_output(runtime, step, "document") {
                    state.outputs.insert("document".into(), doc);
                    return Ok(true);
                } else {
                    state.status = "blocked".into();
                    state.reason = "上游未产生可采纳内容".into();
                }
                Ok(false)
            }
            "attempt.execute" => {
                if !state.outputs.contains_key("attempt") {
                    if let Some(task) = runtime
                        .task_id
                        .as_ref()
                        .and_then(|id| s.tasks.iter().find(|t| &t.id == id))
                    {
                        if !task
                            .dependencies
                            .iter()
                            .all(|dep| s.task(dep).is_ok_and(|t| t.status == Status::Completed))
                        {
                            state.status = "waiting_dependencies".into();
                            state.reason = "等待前置 Task 完成".into();
                            return Ok(false);
                        }
                    }
                    let live = s.projects.iter().any(|p| {
                        p.id == s.feature(&runtime.feature_id).unwrap().project_id
                            && p.execution_mode == "live"
                    });
                    if live
                        && (!s.repository_worktrees.iter().any(|w| w.owner_id == owner)
                            || !s.heads.iter().any(|head| head.id == owner))
                    {
                        state.status = "waiting_dependencies".into();
                        state.reason = "等待受管理 worktree 与代码头就绪".into();
                        return Ok(false);
                    }
                }

                if let Some(aid) = state.outputs.get("attempt").cloned() {
                    let a = s.attempt(&aid)?;
                    let runs = s.latest_runs(Some(&aid), None);
                    let current = runs
                        .iter()
                        .find(|run| run.status == Status::Running)
                        .or_else(|| {
                            runs.iter().find(|run| {
                                matches!(
                                    run.status,
                                    Status::Failed | Status::Interrupted | Status::Cancelled
                                )
                            })
                        })
                        .or_else(|| runs.iter().find(|run| run.status == Status::Queued))
                        .or_else(|| {
                            runs.iter()
                                .rev()
                                .find(|run| run.status == Status::Succeeded)
                        });
                    if let Some(run) = current {
                        state
                            .outputs
                            .insert("session".into(), run.session_id.clone());
                        if s.turns.iter().any(|turn| {
                            turn.run_id.as_ref() == Some(&run.id)
                                && turn.status == TurnStatus::NeedsReconciliation
                        }) && run.status == Status::Running
                        {
                            state.status = "blocked".into();
                            state.reason = "原执行结果待核对，正在停止并封存执行者；停止后可在本步骤技术重试，无需再次采纳提案".into();
                            return Ok(false);
                        }
                    }
                    if [
                        Status::Failed,
                        Status::Interrupted,
                        Status::Cancelled,
                        Status::Rejected,
                    ]
                    .contains(&a.status)
                    {
                        state.status = "failed".into();
                        state.reason = current
                            .and_then(|run| run.result.as_ref())
                            .and_then(|result| result["message"].as_str())
                            .map(str::to_owned)
                            .unwrap_or_else(|| format!("执行 {:?}，需要显式重试或返工", a.status));
                        return Ok(false);
                    }
                    let kind = match phase(step) {
                        "requirement_definition" => "requirement",
                        "solution_planning" => "plan",
                        other => other,
                    };
                    if let Some(d) = s.deliveries.iter().rev().find(|d| {
                        d.attempt_id.as_deref() == Some(&aid) && d.kind == kind && !d.stale
                    }) {
                        state.outputs.insert("delivery".into(), d.id.clone());
                        return Ok(true);
                    }
                    if a.status == Status::WaitingReview {
                        self.workflow_intent(
                            s,
                            state,
                            "delivery.submit",
                            &aid,
                            json!({"kind":kind}),
                        )?;
                    }
                } else {
                    self.workflow_intent(
                        s,
                        state,
                        "attempt.start",
                        &owner,
                        json!({"phase":phase(step)}),
                    )?;
                    if s.repository_worktrees
                        .iter()
                        .find(|w| w.owner_id == owner)
                        .is_some_and(|w| s.run_cancellation_pending(&w.path))
                    {
                        state.status = "waiting_dependencies".into();
                        state.reason = "等待旧 Run 取消确认后再启动".into();
                    }
                }
                Ok(false)
            }
            "delivery.review" | "merge.review" => {
                let delivery = dependencies_output(runtime, step, "delivery").or_else(|| {
                    dependencies_output(runtime, step, "merge").and_then(|mid| {
                        s.deliveries
                            .iter()
                            .rev()
                            .find(|d| d.merge_id.as_deref() == Some(&mid))
                            .map(|d| d.id.clone())
                    })
                });
                let Some(delivery) = delivery else {
                    state.status = "blocked".into();
                    state.reason = "等待冻结交付".into();
                    return Ok(false);
                };
                let d = s.delivery(&delivery)?.clone();
                state.outputs.insert("delivery".into(), delivery.clone());
                if let Some(run) = s
                    .runs
                    .iter()
                    .rev()
                    .find(|run| run.node == format!("review:{delivery}"))
                {
                    state
                        .outputs
                        .insert("session".into(), run.session_id.clone());
                }
                if let Some(mid) = &d.merge_id {
                    state.outputs.insert("merge".into(), mid.clone());
                }
                if s.accepted(&d) {
                    return Ok(true);
                }
                if !crate::model_approvals::reports(s, state).is_empty() {
                    return Ok(true);
                }
                if let Some(review) = s.reviews.iter().rev().find(|r| r.delivery_id == delivery) {
                    if review.decision == Decision::ChangesRequested
                        || review.decision == Decision::Inconclusive
                    {
                        state.status = "failed".into();
                        state.reason = review.reason.clone();
                        return Ok(false);
                    }
                }
                if let Some(run) = s
                    .runs
                    .iter()
                    .rev()
                    .find(|r| r.node == format!("review:{delivery}"))
                    && [Status::Failed, Status::Interrupted, Status::Cancelled]
                        .contains(&run.status)
                {
                    let retry_pending = s.governance.iter().any(|intent| {
                        intent.kind == "workflow_intent"
                            && intent.definition_id == state.id
                            && intent.data["method"] == "run.retry"
                            && intent.data["status"] == "prepared"
                            && intent.data["command"]["target"] == run.id
                    });
                    if !retry_pending {
                        state.status = "failed".into();
                        state.reason = run
                            .result
                            .as_ref()
                            .and_then(|result| result["message"].as_str())
                            .unwrap_or("评审执行失败，需要显式技术重试或返工")
                            .into();
                    }
                    return Ok(false);
                }
                if !s.runs.iter().any(|r| {
                    r.node == format!("review:{delivery}")
                        && [Status::Queued, Status::Running].contains(&r.status)
                }) {
                    self.workflow_intent(s, state, "review.request", &delivery, json!({}))?;
                }
                Ok(false)
            }
            "merge.prepare" => {
                if let Some(mid) = state.outputs.get("merge").cloned() {
                    if let Some(d) = s
                        .deliveries
                        .iter()
                        .rev()
                        .find(|d| d.merge_id.as_deref() == Some(&mid) && !d.stale)
                    {
                        state.outputs.insert("delivery".into(), d.id.clone());
                        return Ok(true);
                    }
                    if [Status::Failed, Status::Interrupted].contains(&s.merge(&mid)?.status) {
                        state.status = "failed".into();
                        state.reason = "候选失效，需要重建".into();
                    }
                } else {
                    self.workflow_intent(s, state, "merge.request", &owner, json!({}))?;
                }
                Ok(false)
            }
            "merge.apply" => {
                let mid = dependencies_output(runtime, step, "merge")
                    .ok_or_else(|| err("blocked", "缺少已评审候选"))?;
                state.outputs.insert("merge".into(), mid.clone());
                if s.merge(&mid)?.status == Status::Completed {
                    return Ok(true);
                }
                if let Some(advance) = s
                    .advances
                    .iter()
                    .find(|a| a.id == mid && a.status == AdvanceStatus::NotApplied)
                {
                    state.status = "blocked".into();
                    state.reason =
                        format!("接纳尚未执行，需显式技术重试或返工：{}", advance.reason);
                    return Ok(false);
                }
                if [Status::Failed, Status::Interrupted, Status::Rejected]
                    .contains(&s.merge(&mid)?.status)
                    && !s.advances.iter().any(|a| {
                        a.id == mid
                            && [
                                AdvanceStatus::Prepared,
                                AdvanceStatus::Dispatched,
                                AdvanceStatus::NeedsReconciliation,
                            ]
                            .contains(&a.status)
                    })
                {
                    state.status = "failed".into();
                    state.reason = "候选失效，请从候选准备返工".into();
                    return Ok(false);
                }
                self.workflow_intent(s, state, "merge.apply", &mid, json!({}))?;
                Ok(false)
            }
            "snapshot.capture" => {
                if let Some(mid) = dependencies_output(runtime, step, "merge") {
                    if let Some(snapshot) = s.snapshots.iter().rev().find(|snap| {
                        snap.feature_id.as_deref() == Some(&runtime.feature_id)
                            && snap.task_id == runtime.task_id
                            && snap.delivery_id.as_ref().is_some_and(|id| {
                                s.delivery(id)
                                    .is_ok_and(|d| d.merge_id.as_deref() == Some(&mid))
                            })
                    }) {
                        state.outputs.insert("snapshot".into(), snapshot.id.clone());
                        return Ok(true);
                    }
                }
                let owner_completed = runtime.task_id.as_ref().map_or_else(
                    || {
                        s.feature(&runtime.feature_id)
                            .is_ok_and(|f| f.status == Status::Completed)
                    },
                    |id| s.task(id).is_ok_and(|t| t.status == Status::Completed),
                );
                if owner_completed {
                    if let Some(snapshot) = s.snapshots.iter().rev().find(|snap| {
                        snap.feature_id.as_deref() == Some(&runtime.feature_id)
                            && snap.task_id == runtime.task_id
                            && snap.delivery_id.is_some()
                    }) {
                        state.outputs.insert("snapshot".into(), snapshot.id.clone());
                        return Ok(true);
                    }
                }
                if state.outputs.contains_key("snapshot") {
                    return Ok(true);
                }
                self.workflow_intent(s, state, "snapshot.save", &owner, json!({}))?;
                Ok(false)
            }
            "tasks.foreach" => {
                if s.runs.iter().any(|r| {
                    r.status == Status::Running
                        && r.cancel_requested
                        && r.attempt_id.as_ref().is_some_and(|id| {
                            s.attempt(id)
                                .is_ok_and(|a| a.feature_id == runtime.feature_id)
                        })
                }) {
                    state.status = "waiting_dependencies".into();
                    state.reason = "等待旧任务 writer 停止".into();
                    return Ok(false);
                }

                let selected_plan = dependencies_output(runtime, step, "proposal");
                let proposal = s
                    .proposals
                    .iter()
                    .rev()
                    .find(|p| {
                        p.target.as_deref() == Some(owner.as_str())
                            && selected_plan.as_ref().is_none_or(|id| &p.id == id)
                            && p.dependencies.iter().all(|id| {
                                !s.documents
                                    .iter()
                                    .any(|d| d.supersedes.as_ref() == Some(id))
                            })
                            && p.status == ProposalStatus::Accepted
                            && matches!(p.content, ProposalContent::TaskPlan { .. })
                    })
                    .cloned();
                let Some(proposal) = proposal else {
                    state.status = "waiting_input".into();
                    state.reason = "需要已采纳的 TaskPlan".into();
                    return Ok(false);
                };
                if proposal.task_instances.is_empty() {
                    self.workflow_intent(
                        s,
                        state,
                        "proposal.instantiate",
                        &proposal.id,
                        json!({}),
                    )?;
                    return Ok(false);
                }
                if runtime.task_plan_revision.as_deref() != Some(&proposal.id) {
                    if runtime.task_plan_revision.is_some() {
                        state.status = "blocked".into();
                        state.reason = "TaskPlan 已变化，请显式返工以创建新任务集合".into();
                        return Ok(false);
                    }
                    runtime.task_plan_revision = Some(proposal.id.clone());
                    runtime.task_members = proposal.task_instances.values().cloned().collect();
                    require(
                        !runtime.task_members.is_empty(),
                        "空 Task 集合不能视为交付完成",
                    )?;
                    let group = record(
                        "workflow_task_group",
                        state.id.clone(),
                        Scope::Feature {
                            project_id: s.feature(&runtime.feature_id)?.project_id.clone(),
                            feature_id: runtime.feature_id.clone(),
                        },
                        "固定任务集合".into(),
                        runtime.generation,
                        None,
                        json!({"task_plan_revision":proposal.id,"members":runtime.task_members,"generation":runtime.generation}),
                        true,
                    );
                    state.outputs.insert("group".into(), group.id.clone());
                    s.governance.push(group);
                }
                for task in runtime.task_members.clone() {
                    if !s.governance.iter().any(|r| {
                        r.kind == "workflow_instance"
                            && r.definition_id == task
                            && r.data["status"] != "cancelled"
                    }) {
                        let r = WorkflowRuntime {
                            owner_id: task.clone(),
                            feature_id: runtime.feature_id.clone(),
                            task_id: Some(task.clone()),
                            parent_id: Some(instance_id.into()),
                            configuration_lock_id: runtime.configuration_lock_id.clone(),
                            workflow_ref: lock
                                .plan
                                .subworkflow_refs
                                .get("task")
                                .cloned()
                                .unwrap_or_default(),
                            status: "active".into(),
                            paused: false,
                            dirty: true,
                            steps: lock
                                .task_plan
                                .steps
                                .iter()
                                .map(|step| WorkflowStepRuntime {
                                    id: id(),
                                    key: step.key.clone(),
                                    round: 1,
                                    status: "pending".into(),
                                    input_hash: String::new(),
                                    outputs: BTreeMap::new(),
                                    reason: String::new(),
                                    revision: 1,
                                    confirmation: None,
                                })
                                .collect(),
                            task_members: vec![],
                            task_plan_revision: None,
                            generation: runtime.generation,
                        };
                        s.governance.push(record(
                            "workflow_instance",
                            task.clone(),
                            Scope::Feature {
                                project_id: s.feature(&runtime.feature_id)?.project_id.clone(),
                                feature_id: runtime.feature_id.clone(),
                            },
                            s.task(&task)?.title.clone(),
                            1,
                            None,
                            json!(r),
                            false,
                        ));
                    }
                }
                Ok(runtime
                    .task_members
                    .iter()
                    .all(|id| s.task(id).is_ok_and(|t| t.status == Status::Completed)))
            }
            "resource.cleanup" => {
                let runs: Vec<_> = s
                    .runs
                    .iter()
                    .filter(|r| {
                        r.attempt_id.as_ref().is_some_and(|id| {
                            s.attempt(id)
                                .is_ok_and(|a| a.feature_id == runtime.feature_id)
                        })
                    })
                    .map(|r| r.id.as_str())
                    .collect();
                let outstanding = s.resources.iter().any(|r| {
                    runs.contains(&r.run_id.as_str())
                        && [Status::Active, Status::CleanupFailed].contains(&r.status)
                });
                if outstanding {
                    state.status = "blocked".into();
                    state.reason = "等待资源释放或清理重试".into();
                }
                Ok(!outstanding)
            }
            "workflow.call" => {
                let outputs: BTreeMap<String, WorkflowPortReference> = serde_json::from_value(
                    step.parameters
                        .get("call_outputs")
                        .cloned()
                        .unwrap_or(json!({})),
                )
                .map_err(|e| err("invalid_workflow", e.to_string()))?;
                for (port, source) in outputs {
                    let Some(value) = runtime
                        .steps
                        .iter()
                        .find(|s| s.key == source.step)
                        .and_then(|s| s.outputs.get(&source.port))
                        .cloned()
                    else {
                        state.status = "blocked".into();
                        state.reason = format!("子流程输出未就绪: {port}");
                        return Ok(false);
                    };
                    state.outputs.insert(port, value);
                }
                Ok(true)
            }
            "branch" | "join" => Ok(true),
            _ => Err(err("unsupported_workflow_action", &step.kind)),
        }
    }

    pub fn pending_workflow_intents(&self) -> Result<Vec<ConfigurationRecord>> {
        let s = self.store.snapshot()?;
        let active: Vec<String> = s
            .governance
            .iter()
            .filter(|r| r.kind == "workflow_instance" && r.data["status"] != "cancelled")
            .filter_map(|r| r.decode::<WorkflowRuntime>().ok())
            .filter(|r| !s.workflow_paused(&r.owner_id))
            .flat_map(|r| {
                r.steps
                    .into_iter()
                    .filter(|step| !["completed", "skipped"].contains(&step.status.as_str()))
                    .map(|step| step.id)
            })
            .collect();
        Ok(s.governance
            .iter()
            .filter(|r| {
                r.kind == "workflow_intent"
                    && r.data["status"] == "prepared"
                    && active.contains(&r.definition_id)
            })
            .take(16)
            .cloned()
            .collect())
    }
    pub fn observe_workflow_intent(&self, intent: &str, result: &Result<Value>) -> Result<()> {
        self.store.transact(None, &mut |s| {
            let old = s.configuration(intent)?.clone();
            require(old.kind == "workflow_intent", "不是工作流动作")?;
            let mut data = old.data.clone();
            match result {
                Ok(value) => {
                    data["status"] = json!("completed");
                    data["result"] = value.clone();
                }
                Err(e) => {
                    data["status"] = if old.data["method"] == "attempt.start"
                        && e.code == "run_cancellation_pending"
                    {
                        json!("prepared")
                    } else {
                        json!("failed")
                    };
                    if e.code == "run_cancellation_pending" {
                        data["waiting_for"] = json!("run_cancellation");
                    }
                    data["error"] = json!(format!("{}: {}", e.code, e.message));
                }
            }
            update_record(
                s.governance.iter_mut().find(|r| r.id == intent).unwrap(),
                data,
            );
            Ok(json!({"recorded":true}))
        })?;
        Ok(())
    }
}
