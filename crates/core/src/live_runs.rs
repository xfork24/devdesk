//! Native engineering stages use frozen role Sessions and managed test executions.
use crate::*;
use std::collections::BTreeMap;
fn candidate_review_evidence(
    merge: &Merge,
    candidate: &str,
    parents: Vec<String>,
    operations: &[RepositoryOperation],
) -> Result<Value> {
    require(
        merge.candidate == candidate && parents == [merge.expected_target.clone()],
        "候选父节点或冻结目标基线不匹配",
    )?;
    let receipts = operations.iter().filter(|op| op.status == RepositoryOperationStatus::Completed && matches!(&op.action,
        RepositoryAction::Candidate{merge:id,..} | RepositoryAction::PublishRef{merge:id,..} | RepositoryAction::PullRequest{merge:id,..} if id == &merge.id)).collect::<Vec<_>>();
    Ok(
        json!({"merge":merge,"candidate":candidate,"parents":parents,"source":"coordinator Git commit graph read","construction_receipts":receipts}),
    )
}
impl Engine {
    pub(crate) fn live_binding(
        &self,
        s: &mut EngineeringState,
        session_id: &str,
        standard: &str,
    ) -> Result<BindingLock> {
        let session = s
            .sessions
            .iter()
            .find(|v| v.id == session_id)
            .cloned()
            .ok_or_else(|| err("blocked", "缺少正式阶段 Session"))?;
        let runtime = session
            .runtime_binding
            .as_ref()
            .ok_or_else(|| err("blocked", "正式阶段缺少冻结运行时"))?;
        let installation = &runtime.installation;
        let harness = if runtime.permissions.is_some() {
            format!(
                "{}:permission:{}",
                installation.id, runtime.configuration_digest
            )
        } else {
            installation.id.clone()
        };
        let sandbox = format!("docker-image:{harness}");
        let image = match &installation.artifact {
            RuntimeArtifact::DockerImage { image_id } => image_id.clone(),
            RuntimeArtifact::OciImage { reference } => reference.clone(),
            _ => return Err(err("blocked", "工程执行要求固定 Linux 容器镜像")),
        };
        if !s.harnesses.iter().any(|h| h.id == harness) {
            s.harnesses.push(devdesk_domain::Harness {
                permission: runtime
                    .permissions
                    .as_ref()
                    .map(|p| PermissionBinding::frozen(&p.harness)),
                id: harness.clone(),
                provider: installation.key.provider.as_str().into(),
                version: installation.key.version.clone(),
                capabilities: vec![
                    "structured-output".into(),
                    "poll".into(),
                    "cancel".into(),
                    "native-session".into(),
                ],
                origin: Origin::External,
            });
        }
        if !s.sandboxes.iter().any(|b| b.id == sandbox) {
            s.sandboxes.push(devdesk_domain::Sandbox {
                permission: runtime
                    .permissions
                    .as_ref()
                    .map(|p| PermissionBinding::frozen(&p.sandbox)),
                id: sandbox.clone(),
                provider: "docker".into(),
                version: "managed-worker/v1".into(),
                platform: format!("{}/{}", installation.key.os, installation.key.arch),
                image: Some(image),
                network: true,
                origin: Origin::External,
            });
        }
        let skills = s
            .definitions
            .iter()
            .find(|d| d.id == session.agent)
            .and_then(|d| d.agent_profile.as_ref())
            .ok_or_else(|| err("blocked", "阶段角色修订不完整"))?
            .skill_revisions
            .clone();
        let binding = BindingLock {
            agent: session.agent,
            skills,
            standard: standard.into(),
            harness,
            sandbox,
        };
        s.validate_binding(&binding)?;
        Ok(binding)
    }
    pub(crate) fn validate_live_sessions(
        &self,
        s: &EngineeringState,
        owner: &str,
        phase: &str,
        map: &BTreeMap<String, Id>,
        step_instance: Option<&str>,
    ) -> Result<()> {
        let stages: Vec<(&str, AgentRole, ToolPolicy)> = match phase {
            "requirement_definition" => vec![(
                "requirement_validate",
                AgentRole::Analyst,
                ToolPolicy::ReadOnly,
            )],
            "solution_planning" => vec![
                ("plan_validate", AgentRole::Architect, ToolPolicy::ReadOnly),
                (
                    "test_plan_validate",
                    AgentRole::Tester,
                    ToolPolicy::ReadOnly,
                ),
            ],
            "planning" => vec![
                ("research", AgentRole::Analyst, ToolPolicy::ReadOnly),
                ("requirement", AgentRole::Analyst, ToolPolicy::ReadOnly),
                ("design", AgentRole::Architect, ToolPolicy::ReadOnly),
                ("task_plan", AgentRole::Architect, ToolPolicy::ReadOnly),
                ("test_plan", AgentRole::Tester, ToolPolicy::ReadOnly),
            ],
            "implementation" => vec![
                ("test_author", AgentRole::Tester, ToolPolicy::TestWrite),
                ("red", AgentRole::Tester, ToolPolicy::ReadOnly),
                ("implement", AgentRole::Developer, ToolPolicy::TaskWrite),
                ("green", AgentRole::Tester, ToolPolicy::ReadOnly),
                ("docs", AgentRole::Developer, ToolPolicy::ReadOnly),
                ("verify", AgentRole::Tester, ToolPolicy::ReadOnly),
            ],
            "closure" => vec![
                ("test_author", AgentRole::Tester, ToolPolicy::TestWrite),
                ("feature_tests", AgentRole::Tester, ToolPolicy::ReadOnly),
                ("docs", AgentRole::Developer, ToolPolicy::ReadOnly),
                ("verify", AgentRole::Tester, ToolPolicy::ReadOnly),
            ],
            _ => return Err(err("blocked", "未知真实 Attempt 阶段")),
        };
        let mut stages: Vec<(String, AgentRole, ToolPolicy)> = stages
            .into_iter()
            .map(|(n, r, p)| (n.into(), r, p))
            .collect();
        if let Some(recipe) = self.workflow_recipe(s, owner, phase, step_instance) {
            stages = recipe
                .into_iter()
                .map(|n| {
                    let policy = if n.key == "test_author" {
                        ToolPolicy::TestWrite
                    } else if n.key == "implement" {
                        ToolPolicy::TaskWrite
                    } else {
                        ToolPolicy::ReadOnly
                    };
                    (n.key, n.role, policy)
                })
                .collect();
        }
        require(map.len() == stages.len(), "真实阶段 Session 映射不完整")?;
        for (node, role, policy) in stages {
            let expected_agent = self.workflow_agent_for(
                s,
                owner,
                "attempt.execute",
                Some(phase),
                Some(&node),
                step_instance,
            )?;
            let sid = map
                .get(&node)
                .ok_or_else(|| err("blocked", "缺少阶段 Session"))?;
            let session = s
                .sessions
                .iter()
                .find(|v| &v.id == sid)
                .ok_or_else(|| err("blocked", "阶段 Session 不存在"))?;
            let profile = s
                .definitions
                .iter()
                .find(|d| d.id == session.agent)
                .and_then(|d| d.agent_profile.as_ref())
                .ok_or_else(|| err("blocked", "阶段 Agent 无结构化角色"))?;
            require(
                session.owner_id == owner
                    && expected_agent
                        .as_ref()
                        .is_none_or(|agent| &session.agent == agent)
                    && profile.role == role
                    && session
                        .runtime_binding
                        .as_ref()
                        .is_some_and(|b| b.tool_policy == policy),
                "阶段 Session 范围、角色或权限不符",
            )?;
        }
        Ok(())
    }
    pub(crate) fn add_live_run(
        &self,
        s: &mut EngineeringState,
        attempt: &str,
        node: &str,
        deps: Vec<String>,
        writes: bool,
    ) -> Result<Id> {
        let a = s.attempt(attempt)?.clone();
        let sid = a
            .live_sessions
            .get(node)
            .ok_or_else(|| err("blocked", "缺少冻结阶段 Session"))?
            .clone();
        let binding = self.live_binding(s, &sid, &a.binding.standard)?;
        let c = a
            .git_context
            .as_ref()
            .ok_or_else(|| err("blocked", "真实 Attempt 缺少代码定位"))?;
        let rid = id();
        s.runs.push(Run {
            id: rid.clone(),
            attempt_id: Some(attempt.into()),
            merge_id: None,
            node: node.into(),
            dependencies: deps,
            status: Status::Queued,
            retry_of: None,
            session_id: sid,
            binding,
            binding_verified: true,
            writes,
            worktree: c.source.worktree.clone(),
            outcome: "external".into(),
            cancel_requested: false,
            started: None,
            finished: None,
            result: None,
            result_artifact: None,
            origin: Origin::External,
        });
        Ok(rid)
    }
}

impl Engine {
    fn delivery_provenance(&self, s: &EngineeringState, d: &Delivery) -> Result<Vec<Value>> {
        let mut queue = d.source_deliveries.clone();
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        while let Some(id) = queue.pop() {
            require(seen.len() < 100, "来源交付链超过上限")?;
            if !seen.insert(id.clone()) {
                continue;
            }
            let source = s.delivery(&id)?;
            require(source.feature_id == d.feature_id, "交付来源跨 Feature")?;
            queue.extend(source.source_deliveries.clone());
            let merges: Vec<_> = s
                .merges
                .iter()
                .filter(|m| m.source_delivery == source.id && m.status == Status::Completed)
                .collect();
            out.push(json!({"delivery":source,"documents":source.documents.iter().filter_map(|id|s.documents.iter().find(|d|&d.id==id)).collect::<Vec<_>>(),"evidence":source.evidence.iter().filter_map(|id|s.runs.iter().find(|r|&r.id==id)).collect::<Vec<_>>(),"reviews":s.reviews.iter().filter(|r|r.delivery_id==source.id).collect::<Vec<_>>(),"completed_merges":merges,"remote_receipts":s.repository_operations.iter().filter(|op|matches!(&op.receipt,Some(RepositoryReceipt::PullRequest{pull_request}) if pull_request.merged)&&matches!(&op.action,RepositoryAction::PullRequest{merge,..} if merges.iter().any(|m|&m.id==merge))).collect::<Vec<_>>() }));
        }
        Ok(out)
    }
    fn live_documents(&self, s: &EngineeringState, r: &Run) -> Result<Vec<Id>> {
        if let Some(aid) = &r.attempt_id {
            let a = s.attempt(aid)?;
            Ok(a.documents.iter().chain(&a.outputs).cloned().collect())
        } else {
            Ok(
                s.delivery(&s.merge(r.merge_id.as_deref().unwrap())?.source_delivery)?
                    .documents
                    .clone(),
            )
        }
    }
    pub(crate) fn poll_live_run(&self, s: &EngineeringState, r: &Run) -> Result<Option<Value>> {
        require(r.origin == Origin::External, "真实执行入口拒绝模拟 Run")?;
        let session = s
            .sessions
            .iter()
            .find(|v| v.id == r.session_id)
            .ok_or_else(|| err("blocked", "真实 Run Session 缺失"))?;
        if ["red", "green", "verify", "feature_tests", "merge_verify"].contains(&r.node.as_str()) {
            return self.poll_live_test(s, r);
        }
        let existing = s.turns.iter().find(|t| t.run_id.as_ref() == Some(&r.id));
        if r.cancel_requested {
            let Some(turn) = existing else {
                return Ok(Some(
                    json!({"status":"cancelled","cancellation_confirmed":true,"not_dispatched":true,"origin":"external"}),
                ));
            };
            if turn.status.active() {
                self.store.transact(None, &mut |state| {
                    let t = state.turns.iter_mut().find(|t| t.id == turn.id).unwrap();
                    t.cancel_requested = true;
                    if t.status == TurnStatus::Queued {
                        t.status = TurnStatus::Cancelled;
                        t.finished = Some(now());
                    }
                    t.revision += 1;
                    Ok(json!({"ok":true}))
                })?;
                return Ok(None);
            }
            require(
                s.executions
                    .iter()
                    .filter(|e| e.run_id.as_ref() == Some(&r.id))
                    .all(|e| e.writer_revoked),
                "取消仍等待真实执行者停止",
            )?;
            return Ok(Some(
                json!({"status":"cancelled","cancellation_confirmed":true,"origin":"external"}),
            ));
        }
        let Some(turn) = existing else {
            if self.run_uses_task(s, r)? {
                let (documents, code, input) = if r.node.starts_with("review:") {
                    let d = s.delivery(r.node.trim_start_matches("review:"))?;
                    let code = FrozenCode {
                        context: git_context(s, &d.feature_id, d.task_id.as_deref())?,
                        revision: d
                            .candidate
                            .clone()
                            .ok_or_else(|| err("blocked", "评审缺少冻结代码"))?,
                    };
                    let base = if let Some(mid) = &d.merge_id {
                        s.merge(mid)?.expected_target.clone()
                    } else {
                        s.attempt(d.attempt_id.as_deref().unwrap())?
                            .source_base
                            .clone()
                            .ok_or_else(|| err("blocked", "评审缺少基线"))?
                    };
                    let provenance = self.delivery_provenance(s, d)?;
                    let documents = d
                        .documents
                        .iter()
                        .cloned()
                        .chain(provenance.iter().flat_map(|p| {
                            p["delivery"]["documents"]
                                .as_array()
                                .into_iter()
                                .flatten()
                                .filter_map(|id| id.as_str().map(str::to_owned))
                        }))
                        .collect::<std::collections::BTreeSet<_>>()
                        .into_iter()
                        .collect::<Vec<_>>();
                    let candidate_provenance = d
                        .merge_id
                        .as_deref()
                        .map(|id| {
                            let merge = s.merge(id)?;
                            candidate_review_evidence(
                                merge,
                                &code.revision,
                                self.git.commit_parents(&code.context, &code.revision)?,
                                &s.repository_operations,
                            )
                        })
                        .transpose()?;
                    let input = json!({"delivery":d,"candidate_provenance":candidate_provenance,"source_provenance":provenance,"diff":{"base":base,"candidate":code.revision,"text":self.git.diff(&code.context,&base,&code.revision)?},"evidence":d.evidence.iter().filter_map(|id|s.runs.iter().find(|r|&r.id==id)).collect::<Vec<_>>()});
                    (documents, code, input)
                } else {
                    let a = s.attempt(
                        r.attempt_id
                            .as_deref()
                            .ok_or_else(|| err("blocked", "任务 Run 缺少 Attempt"))?,
                    )?;
                    let documents = self.live_documents(s, r)?;
                    let code = FrozenCode {
                        context: a
                            .git_context
                            .clone()
                            .ok_or_else(|| err("blocked", "任务缺少冻结代码上下文"))?,
                        revision: self.live_code_revision(s, r)?,
                    };
                    let input = json!({"authorization":{"run":r.id,"node":r.node,"phase":a.phase,"scope":a.scope},"frozen_context":a.context_content,
                        "prior_review_findings":s.reviews.iter().filter(|review|review.decision==Decision::ChangesRequested&&s.deliveries.iter().any(|d|d.id==review.delivery_id&&d.feature_id==a.feature_id&&d.task_id==a.task_id&&d.kind==a.phase)).map(|review|json!({"reason":review.reason,"checks":review.checks})).collect::<Vec<_>>(),
                        "stage_evidence":effective_runs(s,a).iter().filter(|run|run.status==Status::Succeeded).map(|run|json!({"node":run.node,"result":run.result})).collect::<Vec<_>>()});
                    (documents, code, input)
                };
                self.enqueue_live_turn(
                    r,
                    session,
                    &serde_json::to_string(&input)
                        .map_err(|e| err("serialization", e.to_string()))?,
                    &documents,
                    code,
                )?;
                return Ok(None);
            }
            let frozen_specifications = self.execution_specifications(s, r)?;
            if s.turns
                .iter()
                .any(|t| t.session_id == session.id && t.status.active())
            {
                return Ok(None);
            }
            if r.node.starts_with("review:") {
                let d = s.delivery(r.node.trim_start_matches("review:"))?;
                let code = FrozenCode {
                    context: git_context(s, &d.feature_id, d.task_id.as_deref())?,
                    revision: d
                        .candidate
                        .clone()
                        .ok_or_else(|| err("blocked", "交付缺少冻结代码"))?,
                };
                let base = if let Some(mid) = &d.merge_id {
                    s.merge(mid)?.expected_target.clone()
                } else {
                    s.attempt(d.attempt_id.as_deref().unwrap())?
                        .source_base
                        .clone()
                        .ok_or_else(|| err("blocked", "评审缺少差异基线"))?
                };
                let diff = self.git.diff(&code.context, &base, &code.revision)?;
                let provenance = self.delivery_provenance(s, d)?;
                let input=serde_json::to_string(&json!({"delivery":d,"source_provenance":provenance,"diff":{"base":base,"candidate":code.revision,"text":diff},"frozen_inputs":d.documents.iter().filter_map(|id|s.documents.iter().find(|d|&d.id==id)).collect::<Vec<_>>(),"evidence":d.evidence.iter().filter_map(|id|s.runs.iter().find(|r|&r.id==id)).collect::<Vec<_>>(),"specifications":frozen_specifications,"definitions":s.definitions.iter().filter(|v|if let Some(specs)=&frozen_specifications {specs.iter().any(|spec|spec.id==v.id)} else {v.id==r.binding.standard||v.family=="spec"}).collect::<Vec<_>>(),"code":code,"task":self.configured_run_prompt(s,r)?})).map_err(|e|err("serialization",e.to_string()))?;
                let all_documents: std::collections::BTreeSet<Id> = d
                    .documents
                    .iter()
                    .cloned()
                    .chain(provenance.iter().flat_map(|p| {
                        p["delivery"]["documents"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(|id| id.as_str().map(str::to_string))
                    }))
                    .collect();
                self.enqueue_live_turn(
                    r,
                    session,
                    &input,
                    &all_documents.into_iter().collect::<Vec<_>>(),
                    code,
                )?;
                return Ok(None);
            }
            let a = s.attempt(
                r.attempt_id
                    .as_deref()
                    .ok_or_else(|| err("blocked", "Run 缺少 Attempt"))?,
            )?;
            let documents = self.live_documents(s, r)?;
            require(
                (if a.phase == "requirement_definition" {
                    vec!["requirement"]
                } else {
                    vec!["requirement", "design", "test"]
                })
                .iter()
                .all(|kind| {
                    documents
                        .iter()
                        .any(|id| s.documents.iter().any(|d| &d.id == id && d.kind == *kind))
                }),
                "正式规划需要已采纳的 Requirement、Design、TestPlan",
            )?;
            let context = a
                .git_context
                .clone()
                .ok_or_else(|| err("blocked", "规划缺少冻结代码上下文"))?;
            let code = FrozenCode {
                context,
                revision: self.live_code_revision(s, r)?,
            };
            let input=serde_json::to_string(&json!({"specifications":frozen_specifications,"authorization":{"run":r.id,"node":r.node,"phase":a.phase,"scope":a.scope,"role_revision":r.binding.agent,"tool_policy":session.runtime_binding.as_ref().map(|b|b.tool_policy)},"frozen_context":a.context_content,"prior_review_findings":s.reviews.iter().filter(|review|review.decision==Decision::ChangesRequested&&s.deliveries.iter().any(|d|d.id==review.delivery_id&&d.feature_id==a.feature_id&&d.task_id==a.task_id&&d.kind==a.phase)).map(|review|json!({"reason":review.reason,"checks":review.checks})).collect::<Vec<_>>(),"frozen_inputs":documents.iter().filter_map(|id|s.documents.iter().find(|d|&d.id==id)).collect::<Vec<_>>(),"stage_evidence":effective_runs(s,a).iter().filter(|run|run.status==Status::Succeeded).map(|run|json!({"node":run.node,"result":run.result})).collect::<Vec<_>>(),"definitions":s.definitions.iter().filter(|d|d.id==r.binding.agent||d.id==r.binding.standard||r.binding.skills.contains(&d.id)||s.definitions.iter().find(|d|d.id==r.binding.standard).is_some_and(|standard|standard.slots.iter().any(|slot|slot.spec==d.id))||frozen_specifications.as_ref().is_some_and(|specs|specs.iter().any(|spec|spec.id==d.id))).collect::<Vec<_>>(),"code":code,"task":self.configured_run_prompt(s,r)?})).map_err(|e|err("serialization",e.to_string()))?;
            self.enqueue_live_turn(r, session, &input, &documents, code)?;
            return Ok(None);
        };
        if turn.status == TurnStatus::NeedsReconciliation {
            let executions: Vec<_> = s.executions.iter()
                .filter(|e| e.run_id.as_ref() == Some(&r.id)).collect();
            if !executions.is_empty() && executions.iter()
                .all(|e| e.writer_revoked && e.protocol_artifact.is_some())
            {
                return Ok(Some(json!({"status":"interrupted","failure_type":"needs_reconciliation",
                    "message":"原执行结果未知，执行者已停止并封存。请在本步骤执行技术重试；旧输入和结果保留，不会自动重发，也无需再次采纳需求提案。",
                    "turn_id":turn.id,"origin":"external"})));
            }
            return Ok(None);
        }
        if turn.status.active() {
            return Ok(None);
        }
        let executions: Vec<_> = s
            .executions
            .iter()
            .filter(|e| e.run_id.as_ref() == Some(&r.id))
            .collect();
        require(
            !executions.is_empty()
                && executions
                    .iter()
                    .all(|e| e.writer_revoked && e.protocol_artifact.is_some()),
            "Run 缺少执行者停止及协议封存证明",
        )?;
        if turn.status != TurnStatus::Completed {
            return Ok(Some(
                json!({"status":"failed","failure_type":"provider","turn_id":turn.id,"origin":"external"}),
            ));
        }
        let text = s
            .conversation_events
            .iter()
            .rev()
            .find(|e| {
                e.turn_id == turn.id
                    && e.kind == ConversationEventKind::Assistant
                    && e.mode == MessageMode::Full
            })
            .ok_or_else(|| err("adapter_contract", "运行时完成但没有完整答案"))?
            .content
            .trim();
        let clean = text
            .strip_prefix("```json")
            .and_then(|s| s.strip_suffix("```"))
            .unwrap_or(text)
            .trim();
        if ["test_author", "implement", "docs"].contains(&r.node.as_str()) {
            let value: Value = match serde_json::from_str(clean) {
                Ok(v) => v,
                Err(_) => {
                    return Ok(Some(
                        json!({"status":"failed","failure_type":"adapter_contract","message":"阶段输出不是 JSON","origin":"external"}),
                    ));
                }
            };
            if value["completed"] != true
                || !value["summary"]
                    .as_str()
                    .is_some_and(|v| !v.trim().is_empty())
            {
                return Ok(Some(
                    json!({"status":"failed","failure_type":"quality","message":"阶段未完成","origin":"external"}),
                ));
            }
            let mut output = json!({"status":"succeeded","origin":"external","summary":value["summary"],"turn_id":turn.id});
            if r.node == "docs" {
                let d = &value["document"];
                if d["kind"] != "doc"
                    || !["title", "body"]
                        .iter()
                        .all(|k| d[*k].as_str().is_some_and(|v| !v.trim().is_empty()))
                {
                    return Ok(Some(
                        json!({"status":"failed","failure_type":"adapter_contract","origin":"external"}),
                    ));
                }
                output["document"] = d.clone();
                output["code_revision"] = json!(self.live_code_revision(s, r)?);
                return Ok(Some(output));
            }
            let Some(operation) = s.repository_operations.iter().find(|op| op.id == r.id) else {
                let binding = session.runtime_binding.as_ref().unwrap();
                let code = turn
                    .code_view
                    .as_ref()
                    .ok_or_else(|| err("blocked", "写 Run 缺少冻结代码"))?;
                self.store.transact(None, &mut |state| {
                    if !state.repository_operations.iter().any(|op| op.id == r.id) {
                        require(
                            state
                                .executions
                                .iter()
                                .filter(|e| e.run_id.as_ref() == Some(&r.id))
                                .all(|e| e.writer_revoked),
                            "封存前仍有执行者写许可",
                        )?;
                        state.repository_operations.push(RepositoryOperation {
                            id: r.id.clone(),
                            project_id: session.project_id.clone().unwrap(),
                            action: RepositoryAction::Snapshot {
                                run: r.id.clone(),
                                context: code.context.clone(),
                                expected: code.revision.clone(),
                                allowed_paths: binding.writable_paths.clone(),
                                protected_paths: binding.read_only_paths.clone(),
                            },
                            status: RepositoryOperationStatus::Prepared,
                            receipt: None,
                            reason: String::new(),
                            revision: 1,
                        });
                    }
                    Ok(json!({"ok":true}))
                })?;
                return Ok(None);
            };
            if operation.status == RepositoryOperationStatus::NeedsReconciliation {
                return Ok(None);
            }
            if operation.status != RepositoryOperationStatus::Completed {
                return Ok(None);
            }
            let Some(RepositoryReceipt::Snapshot { revision }) = &operation.receipt else {
                return Err(err("adapter_contract", "代码封存缺少回执"));
            };
            output["code_revision"] = json!(revision);
            return Ok(Some(output));
        }
        let answer = match parse_stage_answer(clean) {
            Ok(answer) => answer,
            Err(error) => {
                return Ok(Some(
                    json!({"status":"failed","failure_type":"adapter_contract","message":error.message,"output_contract":STAGE_ANSWER_CONTRACT,"expected_schema":stage_answer_schema(),"turn_id":turn.id,"origin":"external"}),
                ));
            }
        };
        let input_context = serde_json::from_str::<Value>(&turn.input).unwrap_or(Value::Null);
        let approval_required = input_context["output_contract"]["id"] == STAGE_ANSWER_CONTRACT;
        if approval_required && answer.approve.is_none() {
            return Ok(Some(json!({"status":"failed","failure_type":"adapter_contract",
                "message":"正式交付输出 $.approve：必须给出模型 Approve 布尔结论", "turn_id":turn.id,"origin":"external"})));
        }
        let passed = answer.checks.iter().all(|c| c.passed);
        let mut output = json!({"status":if approval_required||passed||r.node.starts_with("review:"){"succeeded"}else{"failed"},"failure_type":if approval_required||passed||r.node.starts_with("review:"){Value::Null}else{json!("quality")},"summary":answer.summary,"checks":answer.checks.iter().map(|c|json!({"key":c.key,"passed":c.passed,"evidence":c.evidence})).collect::<Vec<_>>(),"turn_id":turn.id,"origin":"external"});
        if approval_required {
            output["model_approve"] = json!(passed);
            output["approval_required"] = json!(true);
            output["skill_refs"] = json!(input_context["quality_basis"]["skills"].as_array()
                .map(|skills|skills.iter().filter_map(|skill|skill["id"].as_str()).collect::<Vec<_>>()));
        }
        if r.node.starts_with("review:") {
            output["review_decision"] = json!(if passed { "pass" } else { "changes_requested" });
            output["review_delivery"] = json!(r.node.trim_start_matches("review:"));
        }
        Ok(Some(output))
    }
    fn enqueue_live_turn(
        &self,
        run: &Run,
        session: &Session,
        input: &str,
        documents: &[Id],
        code: FrozenCode,
    ) -> Result<()> {
        let snapshot = self.store.snapshot()?;
        let mut value: Value = serde_json::from_str(input)
            .map_err(|e| err("serialization", e.to_string()))?;
        if let Some(contract) = value.get("task").and_then(|task|task.get("output_contract")).cloned() {
            value["output_contract"] = contract;
        }
        value["quality_basis"] = self.skill_quality_basis(&snapshot, run, documents)?;
        if !["test_author", "implement", "docs"].contains(&run.node.as_str()) {
            value["output_contract"] = stage_answer_output_contract();
        }
        let input = serde_json::to_string(&value).map_err(|e| err("serialization", e.to_string()))?;
        let input = input.as_str();
        require(input.len() <= 512 * 1024, "正式运行输入超出协议限制")?;
        self.store.transact(None, &mut |s| {
            let current = s
                .runs
                .iter()
                .find(|r| r.id == run.id)
                .ok_or_else(|| err("not_found", "Run 不存在"))?;
            require(
                current.status == Status::Running && !current.cancel_requested,
                "派发前 Run 已失效或取消",
            )?;
            if s.turns.iter().any(|t| t.run_id.as_ref() == Some(&run.id)) {
                return Ok(json!({"ok":true}));
            }
            require(
                !s.turns
                    .iter()
                    .any(|t| t.session_id == session.id && t.status.active()),
                "角色 Session 正忙",
            )?;
            let snapshot = self.capture_conversation(
                s,
                session,
                "run_input",
                Some(documents),
                Some(&run.binding),
            )?;
            let owner = if let Some(task) = &session.task_id {
                MaterialOwner::Task {
                    feature: session.feature_id.clone().unwrap(),
                    id: task.clone(),
                }
            } else {
                MaterialOwner::Feature {
                    id: session.feature_id.clone().unwrap(),
                }
            };
            let mut artifact = self.artifacts.seal_scoped(&owner, "run-input", input)?;
            artifact.origin = Origin::External;
            let digest = artifact.sha256.clone();
            let artifact_id = artifact.id.clone();
            s.artifacts.push(artifact);
            let tid = id();
            let sequence = s
                .conversation_events
                .iter()
                .filter(|e| e.session_id == session.id)
                .map(|e| e.sequence)
                .max()
                .unwrap_or(0)
                + 1;
            s.turns.push(ConversationTurn {
                id: tid.clone(),
                project_id: session.project_id.clone().unwrap(),
                session_id: session.id.clone(),
                client_message_id: format!("run:{}", run.id),
                input: input.into(),
                input_digest: digest,
                input_snapshot: Some(snapshot),
                code_view: Some(code.clone()),
                source_revision: revision(s, &session.owner_id),
                status: TurnStatus::Queued,
                external_message: None,
                external_turn: None,
                consumed: false,
                cancel_requested: false,
                started: None,
                finished: None,
                run_id: Some(run.id.clone()),
                revision: 1,
            });
            s.conversation_events.push(ConversationEvent {
                id: id(),
                session_id: session.id.clone(),
                turn_id: tid.clone(),
                sequence,
                deduplication_key: format!("run-input:{}", run.id),
                kind: ConversationEventKind::User,
                content: input.into(),
                artifact_id: Some(artifact_id),
                time: now(),
                mode: MessageMode::Full,
                provider_item: None,
            });
            s.sessions
                .iter_mut()
                .find(|v| v.id == session.id)
                .unwrap()
                .revision += 1;
            event(
                s,
                &run.id,
                "run.input_prepared",
                "Native role input persisted before dispatch",
            );
            self.freeze_turn_task(s, &tid)?;
            Ok(json!({"ok":true}))
        })?;
        Ok(())
    }
}

impl Engine {
    pub(crate) fn drain_live_cleanup(&self) -> Result<()> {
        let state = self.store.snapshot()?;
        for execution in state.executions.iter().filter(|e| {
            e.status == Status::Succeeded && e.writer_revoked && e.protocol_artifact.is_some()
        }) {
            if execution.run_id.as_ref().is_some_and(|id| {
                !state.runs.iter().any(|r| {
                    &r.id == id && r.status != Status::Running && r.result_artifact.is_some()
                })
            }) {
                continue;
            }
            let session = state
                .sessions
                .iter()
                .find(|v| v.id == execution.session_id)
                .unwrap();
            let outcome = if execution.kind == RuntimeExecutionKind::Tests {
                let request = state
                    .effects
                    .iter()
                    .find(|e| e.run_id == execution.operation_id && e.kind == EffectKind::Test)
                    .ok_or_else(|| err("blocked", "测试清理缺少冻结请求"))?;
                let request: TestRequest = serde_json::from_value(request.request["test"].clone())
                    .map_err(|e| err("storage", e.to_string()))?;
                self.tests.cleanup_test(&request)
            } else {
                self.harness.cleanup_worker(session, execution)
            };
            self.store.transact(None, &mut |s| {
                let row = s
                    .executions
                    .iter_mut()
                    .find(|e| e.id == execution.id)
                    .unwrap();
                row.status = if outcome.is_ok() {
                    Status::Released
                } else {
                    Status::CleanupFailed
                };
                event(
                    s,
                    &execution.id,
                    "execution.cleanup",
                    if outcome.is_ok() {
                        "released"
                    } else {
                        "cleanup_failed"
                    },
                );
                Ok(json!({"ok":true}))
            })?;
        }
        Ok(())
    }
}

impl Engine {
    pub(crate) fn discussion_code_view(
        &self,
        s: &EngineeringState,
        session: &Session,
    ) -> Result<Option<FrozenCode>> {
        let Some(project) = s
            .projects
            .iter()
            .find(|p| Some(&p.id) == session.project_id.as_ref() && p.execution_mode == "live")
        else {
            return Ok(None);
        };
        let Some(work) = s
            .repository_worktrees
            .iter()
            .find(|w| w.owner_id == session.owner_id && w.project_id == project.id)
        else {
            return Ok(None);
        };
        let head = if session.scope == Some(SessionScope::Project) {
            format!("main:{}", project.id)
        } else {
            session.owner_id.clone()
        };
        let revision = s
            .heads
            .iter()
            .find(|h| h.id == head)
            .ok_or_else(|| err("blocked", "代码对象尚未核对"))?
            .revision
            .clone();
        let source = CodeLocation {
            owner: session.owner_id.clone(),
            branch: work.branch.clone(),
            worktree: work.path.clone(),
        };
        let context = GitContext {
            project: project.id.clone(),
            workspace: project
                .workspace_definition
                .clone()
                .ok_or_else(|| err("blocked", "仓库定义缺失"))?,
            source: source.clone(),
            target: source,
            origin: Origin::External,
        };
        Ok(Some(FrozenCode { context, revision }))
    }
}

impl Engine {
    fn live_code_revision(&self, s: &EngineeringState, r: &Run) -> Result<String> {
        if let Some(mid) = &r.merge_id {
            return Ok(s.merge(mid)?.candidate.clone());
        }
        let a = s.attempt(
            r.attempt_id
                .as_deref()
                .ok_or_else(|| err("blocked", "Run 缺少 Attempt"))?,
        )?;
        if r.node == "implement" && r.retry_of.is_some() {
            return s
                .heads
                .iter()
                .find(|h| Some(h.id.as_str()) == a.task_id.as_deref())
                .map(|h| h.revision.clone())
                .ok_or_else(|| err("blocked", "重试缺少已核对代码头"));
        }
        if r.node == "red" || r.node == "implement" {
            return a
                .baseline
                .clone()
                .ok_or_else(|| err("blocked", "Red baseline 未封存"));
        }
        if a.phase == "implementation" && ["green", "docs", "verify"].contains(&r.node.as_str()) {
            return effective_runs(s, a)
                .iter()
                .find(|v| v.node == "implement" && v.status == Status::Succeeded)
                .and_then(|v| v.result.as_ref())
                .and_then(|v| v["code_revision"].as_str())
                .map(str::to_string)
                .ok_or_else(|| err("blocked", "实际实现代码未封存"));
        }
        if a.phase == "closure" && r.node != "test_author" {
            return effective_runs(s, a)
                .iter()
                .find(|v| v.node == "test_author" && v.status == Status::Succeeded)
                .and_then(|v| v.result.as_ref())
                .and_then(|v| v["code_revision"].as_str())
                .map(str::to_string)
                .ok_or_else(|| err("blocked", "Feature 测试尚未封存"));
        }
        a.source_base
            .clone()
            .ok_or_else(|| err("blocked", "Attempt 缺少真实基线"))
    }
    fn poll_live_test(&self, s: &EngineeringState, r: &Run) -> Result<Option<Value>> {
        let key = format!("{}:Test", r.id);
        let existing = s.effects.iter().find(|e| e.id == key);
        if r.cancel_requested && existing.is_none() {
            return Ok(Some(
                json!({"status":"cancelled","cancellation_confirmed":true,"not_dispatched":true,"origin":"external"}),
            ));
        }
        let request: TestRequest = if let Some(effect) = existing {
            require(
                effect.request["adapter"]
                    .as_str()
                    .is_some_and(|identity| self.tests.accepts_identity(identity)),
                "测试适配器已更新，旧操作需核对",
            )?;
            serde_json::from_value(effect.request["test"].clone())
                .map_err(|e| err("storage", e.to_string()))?
        } else {
            let documents = self.live_documents(s, r)?;
            let task = r
                .attempt_id
                .as_deref()
                .and_then(|id| s.attempt(id).ok())
                .and_then(|a| a.task_id.as_ref())
                .or_else(|| {
                    r.merge_id
                        .as_deref()
                        .and_then(|id| s.merge(id).ok())
                        .and_then(|m| m.task_id.as_ref())
                });
            let defs: Vec<_> = documents
                .iter()
                .filter_map(|id| {
                    s.documents
                        .iter()
                        .find(|d| &d.id == id && d.kind == "test" && d.task_id.as_ref() == task)
                })
                .cloned()
                .collect();
            require(!defs.is_empty(), "正式测试缺少本范围 Test 定义")?;
            let recipe = defs[0]
                .test_recipe
                .as_ref()
                .ok_or_else(|| err("blocked", "Test 缺少执行配方"))?;
            let context = if let Some(mid) = &r.merge_id {
                Some(git_context(
                    s,
                    &s.merge(mid)?.feature_id,
                    s.merge(mid)?.task_id.as_deref(),
                )?)
            } else {
                s.attempt(r.attempt_id.as_deref().unwrap())?
                    .git_context
                    .clone()
            }
            .ok_or_else(|| err("blocked", "测试缺少代码定位"))?;
            let mut sandbox = s
                .sandboxes
                .iter()
                .find(|v| v.id == r.binding.sandbox)
                .ok_or_else(|| err("blocked", "测试沙箱定义缺失"))?
                .clone();
            sandbox.id = format!("test-image:{}", recipe.image);
            sandbox.version = "node-test/v1".into();
            sandbox.image = Some(recipe.image.clone());
            sandbox.network = false;
            sandbox.permission = Some(PermissionBinding::frozen(
                &isolated_test_permissions().sandbox,
            ));
            let execution = RuntimeExecution {
                permissions: Some(isolated_test_permissions()),
                kind: RuntimeExecutionKind::Tests,
                id: id(),
                session_id: r.session_id.clone(),
                run_id: Some(r.id.clone()),
                operation_id: r.id.clone(),
                container_id: None,
                image_digest: recipe.image.clone(),
                status: Status::Running,
                writer_revoked: false,
                started: Some(now()),
                finished: None,
                protocol_artifact: None,
            };
            let runtime = ExecutionInstance {
                id: execution.id.clone(),
                run_id: r.id.clone(),
                harness: devdesk_domain::Harness {
                    id: format!("node-test:{}", recipe.image),
                    provider: "node-test".into(),
                    version: "node-test/v1".into(),
                    capabilities: vec!["tests".into()],
                    origin: Origin::External,
                    permission: Some(PermissionBinding::frozen(
                        &isolated_test_permissions().harness,
                    )),
                },
                sandbox,
                external_session: None,
                external_environment: None,
                status: Status::Running,
                started: execution.started,
                finished: None,
            };
            let request = TestRequest {
                operation_id: r.id.clone(),
                session_id: Some(r.session_id.clone()),
                phase: r.node.clone(),
                candidate: self.live_code_revision(s, r)?,
                test_definitions: defs.iter().map(|d| d.id.clone()).collect(),
                environment: execution.id.clone(),
                origin: Origin::External,
                definitions: defs,
                code: Some(context),
                runtime: Some(runtime),
            };
            // Persist environment + immutable request together, before starting Docker.
            self.store.transact(None, &mut |state| {
                require(!state.effects.iter().any(|e| e.id == key), "测试操作已存在")?;
                let current = state.runs.iter().find(|v| v.id == r.id).unwrap();
                require(
                    current.status == Status::Running && !current.cancel_requested,
                    "测试派发已取消",
                )?;
                state.executions.push(execution.clone());
                state.effects.push(EffectRecord {
                    id: key.clone(),
                    run_id: r.id.clone(),
                    kind: EffectKind::Test,
                    status: EffectStatus::Pending,
                    request: json!({"test":request,"adapter":self.tests.identity()}),
                    receipt: None,
                    revision: 1,
                    created: now(),
                    updated: now(),
                    reason: "真实测试意图已持久化".into(),
                });
                Ok(json!({"ok":true}))
            })?;
            return Ok(None);
        };
        let output = if r.cancel_requested {
            let stopped = self.tests.stop_test(&request)?;
            require(stopped, "测试执行者未停止")?;
            json!({"status":"cancelled","cancellation_confirmed":true,"writer_revoked":true,"operation_id":r.id,"environment":request.environment,"origin":"external"})
        } else if let Some(receipt) = existing.and_then(|e| e.receipt.as_ref()) {
            receipt.clone()
        } else {
            let Some(value) = self.tests.poll_test(&request)? else {
                return Ok(None);
            };
            let parsed: TestResult = serde_json::from_value(value.clone())
                .map_err(|_| err("adapter_contract", "测试回执结构错误"))?;
            require(
                parsed.operation_id == request.operation_id
                    && parsed.environment == request.environment
                    && parsed.candidate == request.candidate
                    && parsed.test_definitions == request.test_definitions
                    && parsed.test_phase == request.phase
                    && parsed.origin == Origin::External
                    && value["writer_revoked"] == true
                    && value["image_digest"]
                        == request
                            .runtime
                            .as_ref()
                            .unwrap()
                            .sandbox
                            .image
                            .as_ref()
                            .unwrap()
                            .as_str()
                    && value["container_id"]
                        .as_str()
                        .is_some_and(|v| !v.is_empty()),
                "真实测试回执与冻结请求不符",
            )?;
            require(
                r.node != "red"
                    || parsed.assertion_failed && parsed.expected_failure
                    || value["status"] == "failed",
                "Red 必须观察到真实断言失败",
            )?;
            self.observe_effect(&key, value.clone(), true)?;
            value
        };
        self.store.transact(None, &mut |state| {
            let execution = state
                .executions
                .iter()
                .find(|e| e.id == request.environment)
                .unwrap();
            if execution.protocol_artifact.is_some() {
                return Ok(json!({"ok":true}));
            }
            let session = state
                .sessions
                .iter()
                .find(|v| v.id == r.session_id)
                .unwrap();
            let owner = if let Some(task) = &session.task_id {
                MaterialOwner::Task {
                    feature: session.feature_id.clone().unwrap(),
                    id: task.clone(),
                }
            } else {
                MaterialOwner::Feature {
                    id: session.feature_id.clone().unwrap(),
                }
            };
            let mut report = self.artifacts.seal_scoped(
                &owner,
                "test-report",
                &serde_json::to_string_pretty(&output).unwrap(),
            )?;
            report.origin = Origin::External;
            let execution = state
                .executions
                .iter_mut()
                .find(|e| e.id == request.environment)
                .unwrap();
            execution.container_id = output["container_id"].as_str().map(str::to_string);
            execution.writer_revoked = true;
            execution.status = Status::Succeeded;
            execution.finished = Some(now());
            execution.protocol_artifact = Some(report.id.clone());
            state.artifacts.push(report);
            Ok(json!({"ok":true}))
        })?;
        Ok(Some(output))
    }
}

impl Engine {
    /// Shutdown stops owned writers; it never converts an uncertain input into a success or resends it.
    pub fn stop_owned_runtime(&self) -> Result<()> {
        self.store.transact(None,&mut |s|{
            for t in s.turns.iter_mut().filter(|t|t.status.active()&&t.status!=TurnStatus::Queued) {
                t.status=TurnStatus::NeedsReconciliation;t.revision+=1;
                if let Some(session)=s.sessions.iter_mut().find(|v|v.id==t.session_id){session.lifecycle=Some(SessionLifecycle::NeedsReconciliation);session.revision+=1;}
            }
            let runs:Vec<_>=s.runs.iter().filter(|r|r.origin==Origin::External&&r.status==Status::Running).cloned().collect();
            for r in runs {
                let (fid,tid)=if let Some(id)=&r.attempt_id{let a=s.attempt(id)?;(a.feature_id.clone(),a.task_id.clone())}else{let m=s.merge(r.merge_id.as_deref().unwrap())?;(m.feature_id.clone(),m.task_id.clone())};
                let mut artifact=self.artifacts.seal(&fid,tid.as_deref(),"runs",&format!("# Interrupted Run {}\norigin=external\nApplication shutdown; outcome remains unknown; owned writer must be stopped.",r.id))?;artifact.origin=Origin::External;
                let row=s.runs.iter_mut().find(|v|v.id==r.id).unwrap();row.status=Status::Interrupted;row.finished=Some(now());row.result_artifact=Some(artifact.id.clone());s.artifacts.push(artifact);
                if let Some(a)=s.attempts.iter_mut().find(|a|Some(&a.id)==r.attempt_id.as_ref()&&a.status==Status::Running){a.status=Status::Interrupted;a.paused=true;a.revision+=1;}
            }
            Ok(json!({"ok":true}))
        })?;
        self.tick_conversations(0)?;
        self.reconcile_native_tests()?;
        self.drain_live_cleanup()
    }
    pub(crate) fn reconcile_native_tests(&self) -> Result<()> {
        let state = self.store.snapshot()?;
        for execution in state.executions.iter().filter(|e| {
            e.kind == RuntimeExecutionKind::Tests
                && !e.writer_revoked
                && e.run_id.as_ref().is_some_and(|id| {
                    state
                        .runs
                        .iter()
                        .any(|r| &r.id == id && r.status != Status::Running)
                })
        }) {
            let effect = state
                .effects
                .iter()
                .find(|e| e.run_id == execution.operation_id && e.kind == EffectKind::Test)
                .ok_or_else(|| err("blocked", "测试恢复缺少冻结请求"))?;
            require(
                effect.request["adapter"]
                    .as_str()
                    .is_some_and(|identity| self.tests.accepts_identity(identity)),
                "测试执行器已变更，不能核对旧操作",
            )?;
            let request: TestRequest = serde_json::from_value(effect.request["test"].clone())
                .map_err(|e| err("storage", e.to_string()))?;
            require(self.tests.stop_test(&request)?, "旧测试执行者未停止")?;
            self.store.transact(None,&mut |s|{
                let session=s.sessions.iter().find(|v|v.id==execution.session_id).unwrap();let owner=if let Some(tid)=&session.task_id{MaterialOwner::Task{feature:session.feature_id.clone().unwrap(),id:tid.clone()}}else{MaterialOwner::Feature{id:session.feature_id.clone().unwrap()}};
                let mut artifact=self.artifacts.seal_scoped(&owner,"test-stop",&json!({"operation":execution.operation_id,"environment":execution.id,"writer_revoked":true,"outcome":"unknown","mode":"recovery_owned_test_container_stop"}).to_string())?;artifact.origin=Origin::External;
                let row=s.executions.iter_mut().find(|v|v.id==execution.id).unwrap();row.writer_revoked=true;row.finished=Some(now());row.status=Status::Succeeded;row.protocol_artifact=Some(artifact.id.clone());s.artifacts.push(artifact);Ok(json!({"ok":true}))
            })?;
        }
        if !state
            .effects
            .iter()
            .any(|e| e.kind == EffectKind::Test && e.status == EffectStatus::NeedsReconciliation)
        {
            return Ok(());
        }
        // A stopped interrupted test remains unknown. Release only its old intent,
        // so a new operation can be retried without replaying the interrupted one.
        self.store.transact(None, &mut |s| {
            let stopped: Vec<Id> = s
                .executions
                .iter()
                .filter(|e| {
                    e.kind == RuntimeExecutionKind::Tests
                        && e.writer_revoked
                        && e.protocol_artifact.is_some()
                        && e.run_id.as_ref().is_some_and(|id| {
                            s.runs.iter().any(|r| {
                                &r.id == id
                                    && r.origin == Origin::External
                                    && r.status != Status::Running
                            })
                        })
                })
                .filter_map(|e| e.run_id.clone())
                .collect();
            for effect in s.effects.iter_mut().filter(|e| {
                e.kind == EffectKind::Test
                    && e.status == EffectStatus::NeedsReconciliation
                    && stopped.contains(&e.run_id)
            }) {
                effect.status = EffectStatus::Discarded;
                effect.reason = "旧原生测试已停止并封存；保留中断事实，重试使用新操作".into();
                effect.revision += 1;
                effect.updated = now();
            }
            Ok(Value::Null)
        })?;
        Ok(())
    }
}

impl Engine {
    pub(crate) fn retry_live_runs(
        &self,
        s: &mut EngineeringState,
        old: &Run,
        descendants: &[Run],
    ) -> Result<Value> {
        require(old.origin == Origin::External, "原生重试入口拒绝模拟 Run")?;
        require(
            s.executions
                .iter()
                .filter(|e| {
                    e.run_id.as_ref() == Some(&old.id)
                        || descendants.iter().any(|r| e.run_id.as_ref() == Some(&r.id))
                })
                .all(|e| e.writer_revoked),
            "重试前须核对并停止旧执行者",
        )?;
        require(
            !s.turns.iter().any(|t| {
                t.status.active() && t.status != TurnStatus::NeedsReconciliation
                    && (t.run_id.as_ref() == Some(&old.id)
                        || descendants.iter().any(|r| t.run_id.as_ref() == Some(&r.id)))
            }),
            "旧输入结果尚未核对，请先在原会话明确停止，不会重发该输入",
        )?;
        require(s.turns.iter().filter(|t| t.status == TurnStatus::NeedsReconciliation
            && (t.run_id.as_ref() == Some(&old.id)
                || descendants.iter().any(|r| t.run_id.as_ref() == Some(&r.id))))
            .all(|t| s.executions.iter().any(|e| e.session_id == t.session_id
                && e.run_id == t.run_id && e.writer_revoked && e.protocol_artifact.is_some())),
            "未知输入重试前须取得原执行的停止及协议封存证明")?;
        if let Some(aid) = &old.attempt_id {
            let a = s.attempt(aid)?;
            require(
                s.attempt_inputs_current(&a)
                    && ![Status::Accepted, Status::Rejected, Status::Cancelled].contains(&a.status),
                "Attempt 已终结或输入变化，请新建执行",
            )?;
        } else {
            let m = s.merge(old.merge_id.as_deref().unwrap())?;
            require(
                m.status == Status::Running
                    && s.heads
                        .iter()
                        .any(|h| h.id == m.target && h.revision == m.expected_target),
                "候选已失效，请重新构造",
            )?;
        }
        if old.node == "test_author"
            && old.attempt_id.as_deref().is_some_and(|id| {
                s.attempt(id)
                    .is_ok_and(|a| a.phase == "implementation" && a.baseline.is_some())
            })
        {
            return Err(err(
                "blocked",
                "测试作者已经封存 Red baseline，请核对旧回执或创建新 Attempt",
            ));
        }
        let mut originals = vec![old.clone()];
        originals.extend(
            descendants
                .iter()
                .filter(|r| r.status != Status::Queued)
                .cloned(),
        );
        let mut first = None;
        for original in originals {
            require(
                !s.deliveries
                    .iter()
                    .any(|d| d.evidence.contains(&original.id)),
                "重试涉及已冻结证据，必须新 Attempt/Merge",
            )?;
            let session = s
                .sessions
                .iter()
                .find(|v| v.id == original.session_id)
                .unwrap()
                .clone();
            let binding = session
                .runtime_binding
                .clone()
                .ok_or_else(|| err("blocked", "旧运行时锁缺失"))?;
            // Use the old exact binding; global/project/role defaults are intentionally not resolved again.
            let snapshot = self.capture_conversation(s, &session, "run_retry_handoff", None, Some(&original.binding))?;
            let instance = session.runtime_binding.as_ref()
                .and_then(|binding| binding.agent_configuration.as_ref())
                .map(|agent| agent.agent_instance_id.clone())
                .or_else(|| s.governance.iter().find(|r| r.kind == "agent_session" && r.definition_id == session.id)
                    .and_then(|r| r.data["agent_instance_id"].as_str()).map(str::to_owned));
            let mut payload = json!({"agent":session.agent,"binding":binding,
                "force_new":true,"predecessor":session.id,"snapshot_id":snapshot});
            if let Some(instance) = instance { payload["agent_instance_id"] = json!(instance); }
            let command = Command {
                command_id: format!("retry-session:{}", id()),
                target: Some(session.owner_id.clone()),
                expected_revision: revision(s, &session.owner_id),
                payload,
            };
            let opened = self
                .conversation_command("session.open", &command, s)?
                .ok_or_else(|| err("blocked", "无法续用原角色会话"))?;
            self.governance_after_command("session.open", &command, s, &opened)?;
            let mut next = original.clone();
            next.id = id();
            next.retry_of = Some(original.id);
            next.session_id = opened["id"].as_str().unwrap().into();
            self.inherit_task_binding(s, &session.id, &next.session_id)?;
            next.status = Status::Queued;
            next.cancel_requested = false;
            next.started = None;
            next.finished = None;
            next.result = None;
            next.result_artifact = None;
            if first.is_none() {
                first = Some(next.id.clone());
            }
            s.runs.push(next);
        }
        if let Some(aid) = &old.attempt_id {
            let a = s.attempts.iter_mut().find(|a| &a.id == aid).unwrap();
            a.status = Status::Running;
            a.paused = false;
            a.revision += 1;
        }
        let rid = first.unwrap();
        event(
            s,
            &rid,
            "run.native_retry_prepared",
            "new Run with original role/runtime/model/inputs; prior input is never resent",
        );
        Ok(json!({"id":rid,"operation_id":rid}))
    }
}

#[cfg(test)]
mod candidate_review_tests {
    use super::*;
    #[test]
    fn candidate_review_has_exact_target_and_actual_single_parent_evidence() {
        let merge: Merge = serde_json::from_value(json!({"id":"merge","feature_id":"feature","task_id":null,"status":"waiting_review","revision":1,"input_version":1,"source":"source","source_delivery":"delivery","target":"target","expected_target":"base","candidate":"candidate","committed_revision":null,"change_request":null})).unwrap();
        let evidence =
            candidate_review_evidence(&merge, "candidate", vec!["base".into()], &[]).unwrap();
        assert_eq!(evidence["merge"]["expected_target"], "base");
        assert_eq!(evidence["parents"], json!(["base"]));
        assert!(candidate_review_evidence(&merge, "other", vec!["base".into()], &[]).is_err());
        assert!(
            candidate_review_evidence(
                &merge,
                "candidate",
                vec!["base".into(), "source".into()],
                &[]
            )
            .is_err()
        );
        assert!(candidate_review_evidence(&merge, "candidate", vec!["stale".into()], &[]).is_err());
    }
}
