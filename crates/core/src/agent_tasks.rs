//! Trusted task binding and input preparation. Provider dispatch reads immutable snapshots.
use crate::governance::record;
use crate::*;

fn frozen_task_record(s: &EngineeringState, turn: &str) -> Result<Option<FrozenAgentTask>> {
    s.governance
        .iter()
        .find(|r| r.kind == "execution_task" && r.definition_id == turn)
        .map(|r| {
            let task: FrozenAgentTask = serde_json::from_value(r.data["task"].clone())
                .map_err(|e| err("task_contract_mismatch", e.to_string()))?;
            task.validate()?;
            Ok(task)
        })
        .transpose()
}
fn definition(binding: &ConfigurationRecord) -> Result<WorkflowTaskDefinition> {
    let task: WorkflowTaskDefinition = serde_json::from_value(binding.data["definition"].clone())
        .map_err(|e| err("task_contract_mismatch", e.to_string()))?;
    if binding.data["definition_digest"] != task_content_hash(&task) {
        return Err(err("task_contract_mismatch", "任务绑定摘要不匹配"));
    }
    task.decode_step()?;
    Ok(task)
}
pub(crate) fn binding_for_run<'a>(s: &'a EngineeringState, run: &Run) -> Option<&'a ConfigurationRecord> {
    let mut current = Some(run);
    let mut seen = vec![];
    while let Some(r) = current {
        if seen.contains(&r.id) {
            return None;
        }
        seen.push(r.id.clone());
        if let Some(b) = s
            .governance
            .iter()
            .find(|b| b.kind == "execution_task_binding" && b.definition_id == r.id)
        {
            return Some(b);
        }
        current = r
            .retry_of
            .as_ref()
            .and_then(|id| s.runs.iter().find(|r| &r.id == id));
    }
    if run.node.starts_with("review:") {
        return None;
    }
    run.attempt_id.as_ref().and_then(|id| {
        s.governance
            .iter()
            .find(|r| r.kind == "execution_task_binding" && &r.definition_id == id)
    })
}

// A successor retains the task's meaning even in legacy sessions without a v3
// task binding. Stop at a directly bound step so a later discussion is distinct.
fn session_lineage<'a>(s: &'a EngineeringState, sid: &str) -> Vec<&'a Session> {
    let mut lineage = vec![];
    let mut current = s.sessions.iter().find(|session| session.id == sid);
    while let Some(session) = current {
        if lineage
            .iter()
            .any(|previous: &&Session| previous.id == session.id)
        {
            break;
        }
        lineage.push(session);
        current = session.predecessor.as_ref().and_then(|id| {
            s.sessions.iter().find(|previous| {
                &previous.id == id
                    && previous.owner_id == session.owner_id
                    && previous.project_id == session.project_id
                    && previous.agent == session.agent
            })
        });
    }
    lineage
}

pub(crate) fn session_workflow_task(
    s: &EngineeringState,
    sid: &str,
) -> Option<(WorkflowStep, Option<String>)> {
    for session in session_lineage(s, sid) {
        let run = s.runs.iter().rev().find(|run| run.session_id == session.id);
        let binding = s
            .governance
            .iter()
            .find(|r| r.kind == "execution_task_binding" && r.definition_id == session.id)
            .or_else(|| run.and_then(|run| binding_for_run(s, run)));
        if let Some(binding) = binding {
            let definition = definition(binding).ok()?;
            return Some((
                definition.decode_step().ok()?,
                binding.data["recipe_node"]
                    .as_str()
                    .map(str::to_owned)
                    .or_else(|| {
                        run.filter(|r| !r.node.starts_with("review:"))
                            .map(|r| r.node.clone())
                    }),
            ));
        }
        let source = [
            Some(session.id.as_str()),
            run.map(|r| r.id.as_str()),
            run.and_then(|r| r.attempt_id.as_deref()),
        ]
        .into_iter()
        .flatten()
        .find_map(|target| {
            s.governance
                .iter()
                .find(|r| r.kind == "execution_specification" && r.definition_id == target)
        });
        if let Some(source) = source {
            let lock = session
                .feature_id
                .as_deref()
                .and_then(|id| s.feature_configuration(id))?;
            let plan = if session.task_id.is_some() {
                &lock.task_plan
            } else {
                &lock.plan
            };
            let step = plan
                .steps
                .iter()
                .find(|step| Some(step.key.as_str()) == source.data["step"].as_str())?;
            return Some((
                step.clone(),
                run.filter(|r| !r.node.starts_with("review:"))
                    .map(|r| r.node.clone()),
            ));
        }
    }
    None
}

pub(crate) fn session_workflow_context(s: &EngineeringState, sid: &str) -> Option<Value> {
    for session in session_lineage(s, sid) {
        let run = s.runs.iter().rev().find(|run| run.session_id == session.id);
        let source = s
            .governance
            .iter()
            .find(|r| r.kind == "execution_task_binding" && r.definition_id == session.id)
            .or_else(|| run.and_then(|run| binding_for_run(s, run)))
            .or_else(|| {
                [
                    Some(session.id.as_str()),
                    run.map(|r| r.id.as_str()),
                    run.and_then(|r| r.attempt_id.as_deref()),
                ]
                .into_iter()
                .flatten()
                .find_map(|target| {
                    s.governance
                        .iter()
                        .find(|r| r.kind == "execution_specification" && r.definition_id == target)
                })
            })
            .or_else(|| {
                s.governance.iter().find(|r| {
                    r.kind == "agent_session"
                        && r.definition_id == session.id
                        && r.data["workflow_step_instance_id"].is_string()
                })
            });
        if let Some(source) = source {
            let step_id = source.data["step_instance_id"]
                .as_str()
                .or(source.data["workflow_step_instance_id"].as_str())?;
            let agent_session = s
                .governance
                .iter()
                .find(|r| r.kind == "agent_session" && r.definition_id == session.id);
            let recipe = source.data["recipe_node"]
                .as_str()
                .or_else(|| agent_session.and_then(|r| r.data["workflow_recipe_node"].as_str()))
                .or_else(|| {
                    run.filter(|r| !r.node.starts_with("review:"))
                        .map(|r| r.node.as_str())
                });
            return Some(json!({"step_instance_id":step_id,"recipe_node":recipe,
                "workflow_instance_id":source.data["workflow_instance_id"],"step":source.data["step"]}));
        }
    }
    None
}

pub(crate) fn formal_session(s: &EngineeringState, sid: &str) -> bool {
    session_workflow_task(s, sid).map_or_else(
        || {
            session_lineage(s, sid)
                .iter()
                .any(|session| s.runs.iter().any(|run| run.session_id == session.id))
        },
        |(step, _)| step.kind != "agent.interact",
    )
}

pub(crate) fn session_has_formal_run(s: &EngineeringState, sid: &str) -> bool {
    formal_session(s, sid)
        && session_lineage(s, sid)
            .iter()
            .any(|session| s.runs.iter().any(|run| run.session_id == session.id))
}

fn task_base(
    s: &EngineeringState,
    session: &Session,
    step: &WorkflowStep,
    node: Option<&str>,
    lock: Option<&FeatureConfiguration>,
) -> Result<AgentTaskBase> {
    let agent = s
        .definitions
        .iter()
        .find(|a| a.id == session.agent)
        .cloned()
        .ok_or_else(|| err("task_contract_mismatch", "任务 Agent 修订不存在"))?;
    let profile = agent
        .agent_profile
        .as_ref()
        .ok_or_else(|| err("task_contract_mismatch", "任务 Agent 缺少职责"))?;
    let skills = profile
        .skill_revisions
        .iter()
        .map(|id| {
            s.definitions
                .iter()
                .find(|d| &d.id == id && d.family == "skill")
                .cloned()
                .ok_or_else(|| err("task_contract_mismatch", "冻结 Skill 不存在"))
        })
        .collect::<Result<Vec<_>>>()?;
    let role_ref = node
        .and_then(|key| step.execution_recipe.iter().find(|n| n.key == key))
        .and_then(|n| n.role_ref.as_ref())
        .or(step.role_ref.as_ref());
    if let (Some(lock), Some(role)) = (lock, role_ref) {
        require(
            lock.role_agent_bindings.get(role) == Some(&session.agent),
            "任务 Agent 与冻结 Role 绑定不匹配",
        )?;
    }
    let workflow_role=role_ref.and_then(|id|s.governance.iter().filter(|r|r.kind=="role"&&(&r.definition_id==id||&r.id==id))
        .filter(|r|lock.is_none_or(|l|l.definition_hashes.get(&r.id)==Some(&r.content_hash))).max_by_key(|r|r.version)).map(|r|json!({"revision_id":r.id,"content_hash":r.content_hash,"definition_id":r.definition_id,"data":r.data}));
    if role_ref.is_some() && workflow_role.is_none() {
        return Err(err("task_contract_mismatch", "冻结 Role 修订或摘要不可用"));
    }
    let composition: AgentComposition = s.configuration(&session.agent)?.decode()?;
    let agent_role=composition.role_ref.as_deref().map(|id|s.configuration(id)).transpose()?.map(|r|json!({"revision_id":r.id,"content_hash":r.content_hash,"definition_id":r.definition_id,"data":r.data}));
    let role = Some(
        json!({"workflow_role":workflow_role,"agent_role":agent_role,"effective_instructions_source":"immutable AgentRevision.body"}),
    );
    Ok(AgentTaskBase {
        agent,
        skills,
        role,
    })
}
fn input_material(
    s: &EngineeringState,
    id: &str,
    project: &str,
    feature: Option<&str>,
) -> Result<Value> {
    if let Some(d) = s.documents.iter().find(|d| d.id == id) {
        require(
            (d.project_id.as_deref() == Some(project))
                || feature.is_some_and(|fid| s.document_in_feature(d, fid)),
            "任务输入文档不属于当前作用域",
        )?;
        return Ok(json!(d));
    }
    if let Some(p) = s.proposals.iter().find(|p| p.id == id) {
        let session = s
            .sessions
            .iter()
            .find(|v| v.id == p.session_id)
            .ok_or_else(|| err("task_input_missing", "提案来源会话不存在"))?;
        require(
            session.project_id.as_deref() == Some(project)
                && session.feature_id.as_deref() == feature,
            "任务输入提案越权",
        )?;
        return Ok(json!(p));
    }
    if let Some(d) = s.deliveries.iter().find(|d| d.id == id) {
        require(feature == Some(d.feature_id.as_str()), "任务交付输入越权")?;
        return Ok(json!(d));
    }
    if let Some(a) = s.attempts.iter().find(|a| a.id == id) {
        require(
            feature == Some(a.feature_id.as_str()),
            "任务 Attempt 输入越权",
        )?;
        return Ok(json!({"id":a.id,"phase":a.phase,"status":a.status,"documents":a.documents}));
    }
    if let Some(m) = s.merges.iter().find(|m| m.id == id) {
        require(feature == Some(m.feature_id.as_str()), "任务候选输入越权")?;
        return Ok(json!(m));
    }
    if let Some(v) = s.sessions.iter().find(|v| v.id == id) {
        require(
            v.project_id.as_deref() == Some(project) && v.feature_id.as_deref() == feature,
            "任务会话输入越权",
        )?;
        return Ok(json!({"id":v.id,"owner_id":v.owner_id,"agent":v.agent}));
    }
    if let Some(v) = s.snapshots.iter().find(|v| v.id == id) {
        require(
            v.project_id.as_deref() == Some(project)
                || v.feature_id.as_deref() == feature && feature.is_some(),
            "任务快照输入越权",
        )?;
        return Ok(json!({"id":v.id,"kind":v.kind,"documents":v.documents,"content":v.content}));
    }
    Err(err("task_input_missing", format!("无法解析任务输入 {id}")))
}
fn task_sections(def: &WorkflowTaskDefinition, node: Option<&str>) -> Result<Value> {
    let step = def.decode_step()?;
    let node_task = node
        .map(|key| {
            step.execution_recipe
                .iter()
                .find(|n| n.key == key && recipe_node_is_agent(key))
                .and_then(|n| n.task_instructions.clone())
                .ok_or_else(|| err("task_contract_mismatch", "Agent 配方任务缺失"))
        })
        .transpose()?;
    let mut sections = json!({"task":{"instructions":step.task_instructions.as_deref().ok_or_else(||err("task_contract_mismatch","步骤公共任务缺失"))?,"node_instructions":node_task,"required_topics":step.interaction.required_topics},"stage_context":def.stage,"output_contract":agent_task_output_contract(&step,node)});
    if !def.delivery_types.is_empty() {
        sections["delivery_types"] = json!(def.delivery_types);
        sections["delivery_contracts"] = json!({"inputs":step.delivery_inputs,"outputs":step.delivery_outputs});
    }
    Ok(sections)
}
fn render(
    s: &EngineeringState,
    session: &Session,
    turn: &ConversationTurn,
    binding: &ConfigurationRecord,
    node: Option<&str>,
) -> Result<FrozenAgentTask> {
    let def = definition(binding)?;
    let step = def.decode_step()?;

    let lock = session
        .feature_id
        .as_ref()
        .and_then(|id| s.feature_configuration(id));
    let base = task_base(s, session, &step, node, lock.as_ref())?;
    let specs = step
        .spec_refs
        .iter()
        .map(|id| {
            s.definitions
                .iter()
                .find(|d| &d.id == id && d.family == "spec")
                .cloned()
                .ok_or_else(|| err("task_contract_mismatch", "冻结 Spec 不存在"))
        })
        .collect::<Result<Vec<_>>>()?;
    let snapshot = turn
        .input_snapshot
        .as_ref()
        .and_then(|id| s.snapshots.iter().find(|v| &v.id == id));
    let context = s.contexts.iter().find(|c| c.owner_id == session.owner_id);
    let historical_context=snapshot.and_then(|snap|snap.inherited_from.as_ref())
        .and_then(|id|s.snapshots.iter().find(|v|&v.id==id)).map(|snap| {
            let excerpt=snap.content.chars().take(16384).collect::<String>();
            json!({"snapshot_id":snap.id,"kind":snap.kind,"content_excerpt":excerpt,"truncated":excerpt.len()<snap.content.len(),"original_bytes":snap.content.len(),"treatment":"historical material, not current task authority"})
        });
    let documents = snapshot
        .map(|snap| {
            snap.documents
                .iter()
                .map(|id| {
                    input_material(
                        s,
                        id,
                        session.project_id.as_deref().unwrap_or(""),
                        session.feature_id.as_deref(),
                    )
                })
                .collect::<Result<Vec<_>>>()
        })
        .transpose()?
        .unwrap_or_default();
    let runtime = binding.data["workflow_instance_id"]
        .as_str()
        .and_then(|id| s.governance.iter().find(|r| r.id == id))
        .map(|r| r.decode::<WorkflowRuntime>())
        .transpose()?;
    let mut inputs = serde_json::Map::new();
    for (name, reference) in &step.inputs {
        let id = runtime
            .as_ref()
            .and_then(|r| r.steps.iter().find(|v| v.key == reference.step))
            .and_then(|v| v.outputs.get(&reference.port))
            .ok_or_else(|| {
                err(
                    "task_input_missing",
                    format!("{}: 缺少输入 {name}", def.path),
                )
            })?;
        inputs.insert(name.clone(),json!({"reference":reference,"id":id,"material":input_material(s,id,session.project_id.as_deref().unwrap_or(""),session.feature_id.as_deref())?}));
    }
    let mut content = json!({"renderer_version":def.renderer_version,
        "source":{"workflow_revision_id":def.workflow_revision_id,"path":def.path,"source_step":def.source_step,"feature_lock_id":binding.data["feature_lock_id"],"workflow_instance_id":binding.data["workflow_instance_id"],"step_instance_id":binding.data["step_instance_id"],"round":binding.data["round"],"recipe_node":node,"project_id":session.project_id,"feature_id":session.feature_id,"task_id":session.task_id,"session_id":session.id,"run_id":turn.run_id,"turn_id":turn.id},
"specifications":specs,"quality_basis":{"skills":base.skills,"specifications":specs},"input":if turn.run_id.is_some(){serde_json::from_str::<Value>(&turn.input).unwrap_or(json!(turn.input))}else{json!(turn.input)},
        "inputs":inputs,"materials":documents,"context":context,"historical_context":historical_context,"code_view":turn.code_view,
        "authorization":session.runtime_binding.as_ref().map(|b|json!({"permissions":b.permissions,"tool_policy":b.tool_policy,"work_type":b.work_type,"writable_paths":b.writable_paths,"read_only_paths":b.read_only_paths})),
        "output_contract":agent_task_output_contract(&step,node)});
    content
        .as_object_mut()
        .unwrap()
        .extend(task_sections(&def, node)?.as_object().unwrap().clone());
    FrozenAgentTask::new(def, base, content)
}
impl Engine {
    pub(crate) fn freeze_action_tasks(
        &self,
        s: &mut EngineeringState,
        method: &str,
        c: &Command,
        result: &Value,
    ) -> Result<()> {
        if !["session.open", "attempt.start", "review.request"].contains(&method) {
            return Ok(());
        }
        let Some(instance) = c.payload["workflow_step_instance_id"].as_str() else {
            return Ok(());
        };
        let Some((runtime_id, runtime, step_state)) = s
            .governance
            .iter()
            .filter(|r| r.kind == "workflow_instance")
            .filter_map(|r| {
                r.decode::<WorkflowRuntime>().ok().and_then(|v| {
                    v.steps
                        .iter()
                        .find(|v| v.id == instance)
                        .cloned()
                        .map(|step| (r.id.clone(), v, step))
                })
            })
            .next()
        else {
            return Err(err("invalid_workflow", "任务步骤实例不存在"));
        };
        let lock = s
            .feature_configuration(&runtime.feature_id)
            .ok_or_else(|| err("task_contract_mismatch", "Feature 配置锁不存在"))?;
        if lock.compiler_version < 3 {
            return Ok(());
        }
        let spec = lock
            .specification_snapshot
            .iter()
            .find(|b| {
                let path = if runtime.task_id.is_some() {
                    b.path.strip_prefix("tasks/")
                } else {
                    Some(b.path.as_str())
                };
                b.step == step_state.key
                    && path.is_some_and(|path| {
                        let (prefix, key) = path.rsplit_once('/').unwrap_or(("", path));
                        crate::workflow_definition::namespace(prefix, key) == step_state.key
                    })
            })
            .ok_or_else(|| err("task_contract_mismatch", "任务步骤不在冻结编译闭包中"))?;
        let Some(def) = &spec.task else {
            return Ok(());
        };
        let node = c.payload["workflow_recipe_node"].as_str();
        if let Some(node) = node {
            let step = def.decode_step()?;
            require(
                step.kind == "attempt.execute"
                    && step.execution_recipe.iter().any(|n| n.key == node),
                "配方节点不属于冻结步骤",
            )?;
        }
        if method == "session.open"
            && def.decode_step()?.kind == "attempt.execute"
            && node.is_none()
        {
            return Err(err(
                "task_contract_mismatch",
                "正式配方 Session 必须明确 Agent 节点",
            ));
        }

        if node.is_some_and(|n| !recipe_node_is_agent(n)) {
            return Ok(());
        }
        let target = text(result, "id")?;
        let data = json!({"definition":def,"definition_digest":task_content_hash(def),"feature_lock_id":runtime.configuration_lock_id,"workflow_instance_id":runtime_id,"step_instance_id":instance,"round":step_state.round,"recipe_node":node});
        if let Some(old) = s
            .governance
            .iter()
            .find(|r| r.kind == "execution_task_binding" && r.definition_id == target)
        {
            if old.data != data {
                return Err(err(
                    "task_contract_mismatch",
                    "此执行已绑定其他任务，请创建独立步骤会话",
                ));
            }
            return Ok(());
        }
        s.governance.push(record(
            "execution_task_binding",
            target,
            Scope::Feature {
                project_id: s.feature(&runtime.feature_id)?.project_id.clone(),
                feature_id: runtime.feature_id,
            },
            "冻结步骤任务".into(),
            1,
            None,
            data,
            true,
        ));
        Ok(())
    }
    pub(crate) fn freeze_turn_task(&self, s: &mut EngineeringState, turn_id: &str) -> Result<()> {
        if frozen_task_record(s, turn_id)?.is_some() {
            return Ok(());
        }
        let turn = s
            .turns
            .iter()
            .find(|t| t.id == turn_id)
            .cloned()
            .ok_or_else(|| err("not_found", "任务 Turn 不存在"))?;
        let session = s
            .sessions
            .iter()
            .find(|v| v.id == turn.session_id)
            .cloned()
            .ok_or_else(|| err("not_found", "任务 Session 不存在"))?;
        let run = turn
            .run_id
            .as_ref()
            .and_then(|id| s.runs.iter().find(|r| &r.id == id));
        let binding = if let Some(run) = run {
            binding_for_run(s, run)
        } else {
            s.governance
                .iter()
                .find(|r| r.kind == "execution_task_binding" && r.definition_id == session.id)
        }
        .cloned();
        let Some(binding) = binding else {
            if (run.is_some()
                || s.governance
                    .iter()
                    .any(|r| r.kind == "agent_session_use" && r.definition_id == session.id))
                && session
                    .feature_id
                    .as_ref()
                    .and_then(|id| s.feature_configuration(id))
                    .is_some_and(|l| l.compiler_version >= 3)
            {
                return Err(err(
                    "task_contract_mismatch",
                    "v3 执行缺少冻结任务来源，不能回退旧提示",
                ));
            }
            return Ok(());
        };
        let node = run
            .filter(|r| !r.node.starts_with("review:"))
            .map(|r| r.node.as_str())
            .or(binding.data["recipe_node"].as_str());
        let previous = run
            .and_then(|r| r.retry_of.as_ref())
            .and_then(|id| s.turns.iter().find(|t| t.run_id.as_ref() == Some(id)))
            .map(|t| frozen_task_record(s, &t.id))
            .transpose()?
            .flatten();
        let task = if let Some(previous) = previous {
            require(
                binding.data["definition_digest"] == previous.definition_digest,
                "重试改变了任务定义",
            )?;
            let mut content = previous.content.clone();
            content["source"]["run_id"] = json!(turn.run_id);
            if content["input"]["authorization"].is_object() {
                content["input"]["authorization"]["run"] = json!(turn.run_id);
            }

            content["source"]["turn_id"] = json!(turn.id);
            content["source"]["session_id"] = json!(session.id);
            content["retry_of"] = json!(run.and_then(|r| r.retry_of.as_ref()));
            FrozenAgentTask::new(previous.definition, previous.base, content)?
        } else {
            render(s, &session, &turn, &binding, node)?
        };
        let owner = if let Some(tid) = &session.task_id {
            MaterialOwner::Task {
                feature: session.feature_id.clone().unwrap(),
                id: tid.clone(),
            }
        } else {
            MaterialOwner::Feature {
                id: session
                    .feature_id
                    .clone()
                    .ok_or_else(|| err("invalid_scope", "任务缺少 Feature"))?,
            }
        };
        let mut artifact = self.artifacts.seal_scoped(
            &owner,
            "agent-task",
            &serde_json::to_string(&task).map_err(|e| err("serialization", e.to_string()))?,
        )?;
        artifact.origin = Origin::External;
        let data = json!({"task":task,"source_binding_id":binding.id,"artifact_id":artifact.id});
        s.artifacts.push(artifact);
        s.governance.push(record(
            "execution_task",
            turn.id,
            Scope::Feature {
                project_id: turn.project_id,
                feature_id: session.feature_id.unwrap(),
            },
            "已准备 Agent 任务".into(),
            1,
            None,
            data,
            true,
        ));
        Ok(())
    }
    pub(crate) fn frozen_turn_task(
        &self,
        s: &EngineeringState,
        turn: &str,
    ) -> Result<Option<FrozenAgentTask>> {
        frozen_task_record(s, turn)
    }
    pub(crate) fn run_uses_task(&self, s: &EngineeringState, run: &Run) -> Result<bool> {
        if binding_for_run(s, run).is_some() {
            return Ok(true);
        }
        let feature = run
            .attempt_id
            .as_ref()
            .and_then(|id| s.attempt(id).ok())
            .map(|a| a.feature_id.as_str())
            .or_else(|| {
                run.merge_id
                    .as_ref()
                    .and_then(|id| s.merge(id).ok())
                    .map(|m| m.feature_id.as_str())
            });
        if feature
            .and_then(|id| s.feature_configuration(id))
            .is_some_and(|l| l.compiler_version >= 3)
        {
            return Err(err("task_contract_mismatch", "v3 Run 缺少任务绑定"));
        }
        Ok(false)
    }
    pub(crate) fn inherit_task_binding(
        &self,
        s: &mut EngineeringState,
        old: &str,
        next: &str,
    ) -> Result<()> {
        for kind in ["execution_task_binding", "execution_specification"] {
            if let Some(source) = s
                .governance
                .iter()
                .rev()
                .find(|r| r.kind == kind && r.definition_id == old)
                .cloned()
            {
                if s.governance
                    .iter()
                    .any(|r| r.kind == kind && r.definition_id == next)
                {
                    return Err(err("task_contract_mismatch", "后继会话已有其他冻结任务"));
                }
                let mut r = record(
                    kind,
                    next.into(),
                    source.scope,
                    source.name,
                    1,
                    None,
                    source.data,
                    true,
                );
                r.id = id();
                s.governance.push(r);
            }
        }
        let source_step = s
            .governance
            .iter()
            .find(|r| r.kind == "execution_task_binding" && r.definition_id == next)
            .and_then(|r| r.data["step_instance_id"].as_str())
            .map(str::to_owned);
        if let Some(source_step) = source_step {
            for record in s
                .governance
                .iter_mut()
                .filter(|r| r.kind == "workflow_instance")
            {
                let mut runtime: WorkflowRuntime = record.decode()?;
                if let Some(step) = runtime.steps.iter_mut().find(|v| {
                    v.id == source_step
                        && !["completed", "skipped"].contains(&v.status.as_str())
                        && v.outputs.get("session").is_some_and(|id| id == old)
                }) {
                    step.outputs.insert("session".into(), next.into());
                    step.revision += 1;
                    runtime.dirty = true;
                    crate::governance::update_record(record, json!(runtime));
                }
            }
        }
        Ok(())
    }
    pub(crate) fn task_query(&self, method: &str, p: &Value) -> Option<Result<Value>> {
        if !["query.workflow_task_preview", "query.execution_task"].contains(&method) {
            return None;
        }
        Some((|| {
            let s = self.store.snapshot()?;
            if method == "query.execution_task" {
                if p["turn_id"].is_string() == p["run_id"].is_string() {
                    return Err(err("invalid_params", "请指定一个 Turn 或 Run"));
                }
                if let Some(id) = p["run_id"].as_str() {
                    let run = s
                        .runs
                        .iter()
                        .find(|r| r.id == id)
                        .ok_or_else(|| err("not_found", "Run 不存在"))?;
                    if !s.turns.iter().any(|t| t.run_id.as_deref() == Some(id)) {
                        let system = ["red", "green", "verify", "feature_tests", "merge_verify"]
                            .contains(&run.node.as_str());
                        return Ok(
                            json!({"run_id":id,"turn_id":null,"task":null,"status":if system{"system_execution"}else{"not_prepared"},"executor":if system{"test_runner"}else{"agent"}}),
                        );
                    }
                }
                let turn = p["turn_id"]
                    .as_str()
                    .or_else(|| {
                        p["run_id"].as_str().and_then(|id| {
                            s.turns
                                .iter()
                                .find(|t| t.run_id.as_deref() == Some(id))
                                .map(|t| t.id.as_str())
                        })
                    })
                    .ok_or_else(|| err("invalid_params", "需要当前项目的 Turn 或 Run"))?;
                let t = s
                    .turns
                    .iter()
                    .find(|t| t.id == turn)
                    .ok_or_else(|| err("not_found", "Turn 不存在"))?;
                let task = frozen_task_record(&s, turn)?;
                return Ok(
                    json!({"turn_id":t.id,"run_id":t.run_id,"legacy":task.is_none(),"task":task}),
                );
            }
            let mut wf = if let Some(doc) = p.get("document") {
                record(
                    "workflow",
                    "preview".into(),
                    self.store
                        .project_id()
                        .map(|project_id| Scope::Project { project_id })
                        .unwrap_or(Scope::Global),
                    "任务预览".into(),
                    1,
                    None,
                    doc.clone(),
                    true,
                )
            } else {
                s.configuration(&text(p, "id")?)?.clone()
            };
            if p.get("document").is_some() {
                wf.id = p["id"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .unwrap_or("draft-preview")
                    .into();
            }
            if wf.kind == "workflow_draft" {
                wf.data = wf.data["document"].clone();
            }
            let result = crate::workflow_definition::compile(&s, &wf, None, &wf.scope)?;
            let path = text(p, "path")?;
            let binding = result
                .specifications
                .iter()
                .find(|b| b.path == path)
                .ok_or_else(|| err("not_found", "步骤完整路径不存在"))?;
            let Some(def) = &binding.task else {
                return Ok(
                    json!({"legacy":wf.data["schema_version"]!=3,"executor":"system_or_legacy","task":null,"scope_checked":false}),
                );
            };
            let step = def.decode_step()?;
            let node = p["recipe_node"].as_str();
            if let Some(node) = node {
                require(
                    step.kind == "attempt.execute"
                        && step.execution_recipe.iter().any(|n| n.key == node),
                    "预览配方节点不属于当前步骤",
                )?;
            }
            if step.kind == "attempt.execute" && node.is_none() {
                return Ok(
                    json!({"definition":def,"definition_digest":task_content_hash(def),"recipe_nodes":step.execution_recipe,"scope_checked":false,"diagnostics":["选择 Agent 配方节点预览完整任务"]}),
                );
            }
            if node.is_some_and(|n| !recipe_node_is_agent(n)) {
                return Ok(json!({"executor":"test_runner","task":null,"scope_checked":false}));
            }
            let role = node
                .and_then(|n| step.execution_recipe.iter().find(|v| v.key == n))
                .and_then(|n| n.role_ref.as_ref())
                .or(step.role_ref.as_ref());
            let agent = p["agent_revision_id"]
                .as_str()
                .map(str::to_owned)
                .or_else(|| {
                    self.store
                        .project_id()
                        .and_then(|pid| {
                            s.governance
                                .iter()
                                .filter(|r| {
                                    r.kind == "project_configuration" && r.definition_id == pid
                                })
                                .max_by_key(|r| r.version)
                        })
                        .and_then(|r| {
                            role.and_then(|role| r.data["role_agent_bindings"][role].as_str())
                        })
                        .map(str::to_owned)
                });
            let specs = step
                .spec_refs
                .iter()
                .filter_map(|id| s.definitions.iter().find(|d| &d.id == id))
                .collect::<Vec<_>>();
            let mut diagnostics = vec!["配置预览尚未冻结运行授权和实际材料".to_owned()];
            let base = if let Some(agent_id) = agent {
                let a: AgentComposition = s.configuration(&agent_id)?.decode()?;
                let identity = a
                    .role_ref
                    .as_deref()
                    .map(|id| {
                        crate::configuration::role_record(&s, id).map(|r| r.definition_id.clone())
                    })
                    .transpose()?;
                require(identity.as_ref() == role, "预览 Agent 不属于当前 Role")?;
                crate::governance::validate_composition(&s, &a, &wf.scope)?;
                let fake = Session {
                    id: "preview".into(),
                    owner_id: "preview".into(),
                    agent: agent_id,
                    harness: String::new(),
                    sandbox: String::new(),
                    snapshot_id: None,
                    instance: None,
                    origin: Origin::External,
                    project_id: self.store.project_id(),
                    feature_id: None,
                    task_id: None,
                    scope: Some(SessionScope::Project),
                    lifecycle: None,
                    runtime_binding: None,
                    external_session: None,
                    predecessor: None,
                    revision: 1,
                };
                Some(task_base(&s, &fake, &step, node, None)?)
            } else {
                diagnostics.push("尚未绑定实际 Agent，Role/Skill 将在项目配置确定后解析".into());
                None
            };
            let mut preview = task_sections(def, node)?;
            preview.as_object_mut().unwrap().extend(json!({"definition":def,"definition_digest":task_content_hash(def),"base_context":base,"specifications":specs,"inputs":step.inputs,"scope_checked":false,"side_effects":false,"diagnostics":diagnostics}).as_object().unwrap().clone());
            Ok(preview)
        })())
    }
}
