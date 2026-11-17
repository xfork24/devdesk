//! Scoped, read-only navigation over existing frozen workflow facts.
use crate::*;
use std::collections::{BTreeMap, BTreeSet};

pub(crate) struct WorkflowReadScope {
    pub project: String,
    pub owners: BTreeMap<String, (&'static str, String)>,
    pub instances: BTreeSet<String>,
    pub current_instances: BTreeSet<String>,
}

pub(crate) fn workflow_read_scope(
    state: &EngineeringState,
    p: &Value,
) -> Result<WorkflowReadScope> {
    let owner = text(p, "owner_id")?;
    let work = work_reference(state, &owner)?;
    let project = work
        .scope(state)?
        .project_id()
        .ok_or_else(|| err("scope_violation", "工作缺少项目"))?
        .to_owned();
    if p["project_id"].as_str().is_some_and(|id| id != project) {
        return Err(err("scope_violation", "工作不属于当前项目"));
    }
    let mut owners = BTreeMap::new();
    match work {
        WorkReference::Feature { id } => {
            owners.insert(id.clone(), ("feature", state.feature(&id)?.title.clone()));
            for task in state.tasks.iter().filter(|task| task.feature_id == id) {
                owners.insert(task.id.clone(), ("task", task.title.clone()));
            }
        }
        WorkReference::Task { id } => {
            owners.insert(id.clone(), ("task", state.task(&id)?.title.clone()));
        }
        WorkReference::Project { id } => {
            let row = state
                .projects
                .iter()
                .find(|row| row.id == id)
                .ok_or_else(|| err("not_found", "项目不存在"))?;
            owners.insert(id.clone(), ("project", row.title.clone()));
            for feature in state
                .features
                .iter()
                .filter(|feature| feature.project_id == id)
            {
                owners.insert(feature.id.clone(), ("feature", feature.title.clone()));
                for task in state
                    .tasks
                    .iter()
                    .filter(|task| task.feature_id == feature.id)
                {
                    owners.insert(task.id.clone(), ("task", task.title.clone()));
                }
            }
        }
    }
    let mut instances: BTreeSet<String> = owners
        .keys()
        .filter_map(|id| crate::standards::current_work_instance(state, id))
        .map(|row| row.id.clone())
        .collect();
    loop {
        let executions: BTreeSet<&str> = state
            .governance
            .iter()
            .filter(|row| instances.contains(&row.id))
            .flat_map(|row| row.data["steps"].as_array().into_iter().flatten())
            .filter_map(|step| step["id"].as_str())
            .collect();
        let children: Vec<String> = state
            .governance
            .iter()
            .filter(|row| {
                row.r#type == "workflow_instance"
                    && row.scope.project_id() == Some(project.as_str())
                    && row.data["owner_id"]
                        .as_str()
                        .is_some_and(|id| owners.contains_key(id))
                    && row.data["parent_execution_id"]
                        .as_str()
                        .is_some_and(|id| executions.contains(id))
            })
            .map(|row| row.id.clone())
            .collect();
        let before = instances.len();
        instances.extend(children);
        if before == instances.len() {
            break;
        }
    }
    let current_instances = instances.clone();
    // Earlier workflow roots and their children remain evidence for the same
    // work scope even after a successor becomes the current workflow.
    instances.extend(
        state
            .governance
            .iter()
            .filter(|row| {
                row.r#type == "workflow_instance"
                    && row.scope.project_id() == Some(project.as_str())
                    && row.data["owner_id"]
                        .as_str()
                        .is_some_and(|owner| owners.contains_key(owner))
            })
            .map(|row| row.id.clone()),
    );
    Ok(WorkflowReadScope {
        project,
        owners,
        instances,
        current_instances,
    })
}

pub(crate) struct ReadExecution<'a> {
    pub instance: &'a ConfigurationRecord,
    pub execution: &'a Value,
    pub historical: bool,
}
pub(crate) fn read_executions<'a>(
    state: &'a EngineeringState,
    scope: &WorkflowReadScope,
) -> Vec<ReadExecution<'a>> {
    let mut result = vec![];
    for instance in state.governance.iter().filter(|row| {
        scope.instances.contains(&row.id) && row.scope.project_id() == Some(scope.project.as_str())
    }) {
        result.extend(
            instance.data["steps"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|execution| ReadExecution {
                    instance,
                    execution,
                    historical: !scope.current_instances.contains(&instance.id),
                }),
        );
        result.extend(
            state
                .governance
                .iter()
                .filter(|row| {
                    row.r#type == "workflow_execution_history"
                        && row.scope.project_id() == Some(scope.project.as_str())
                        && row.data["workflow_instance_id"] == instance.id
                })
                .map(|row| ReadExecution {
                    instance,
                    execution: &row.data,
                    historical: true,
                }),
        );
    }
    result
}
pub(crate) fn frozen_step<'a>(
    state: &'a EngineeringState,
    entry: &ReadExecution,
) -> Result<(&'a Value, &'a Value)> {
    let plan = state.configuration(
        entry.execution["plan_revision_id"]
            .as_str()
            .ok_or_else(|| err("not_found", "执行未记录计划引用"))?,
    )?;
    require(
        plan.r#type == "workflow_plan_revision"
            && plan.scope == entry.instance.scope
            && plan.data["instance_id"] == entry.instance.id,
        "执行计划不属于此流程",
    )?;
    let plan = &plan.data["plan"];
    let step = plan["document"]["steps"]
        .as_array()
        .and_then(|steps| {
            steps
                .iter()
                .find(|step| step["node_id"] == entry.execution["node_id"])
        })
        .ok_or_else(|| err("not_found", "冻结步骤不可用"))?;
    Ok((plan, step))
}
fn session_actor(state: &EngineeringState, session: &Session) -> Value {
    let binding = session
        .runtime_binding
        .as_ref()
        .and_then(|binding| binding.agent_configuration.as_ref());
    let agent_ref = binding
        .map(|binding| binding.agent_revision_id.as_str())
        .unwrap_or(&session.agent);
    let name = state
        .configuration(agent_ref)
        .ok()
        .map(|agent| agent.name.as_str())
        .unwrap_or(agent_ref);
    json!({"agent_instance_id":binding.map(|binding|&binding.agent_instance_id),"agent_revision_id":agent_ref,"name":name,"session_id":session.id,"role_ref":state.configuration(agent_ref).ok().and_then(|agent|agent.refs.get("role_ref"))})
}
impl Engine {
    pub(crate) fn workflow_contexts(&self, p: &Value) -> Result<Value> {
        let state = self.store.snapshot()?;
        let scope = workflow_read_scope(&state, p)?;
        if let Some(id) = p["context_id"].as_str() {
            let context = state
                .contexts
                .iter()
                .find(|context| context.id == id && scope.owners.contains_key(&context.owner_id))
                .ok_or_else(|| err("not_found", "手工背景不属于当前工作范围"))?;
            return Ok(json!({"context":context,"source":"current_manual_context"}));
        }
        let mut groups: BTreeMap<String, Value> = BTreeMap::new();
        let mut executions = Vec::new();
        let mut seen_turns = BTreeSet::new();
        for entry in read_executions(&state, &scope) {
            if p["instance_id"]
                .as_str()
                .is_some_and(|id| entry.instance.id != id)
            {
                continue;
            }
            let metadata = execution_metadata(&state, &scope, &entry)?;
            if metadata["context_available"] != true {
                continue;
            }
            let (_, step) = frozen_step(&state, &entry)?;
            executions.push(metadata);
            let sessions: Vec<_> = state
                .sessions
                .iter()
                .filter(|session| {
                    session.project_id.as_deref() == Some(scope.project.as_str())
                        && session.scope.is_some()
                        && (entry.execution["session_id"] == session.id
                            || crate::agent_tasks::session_matches_context(
                                &state,
                                session,
                                entry.execution["id"].as_str().unwrap_or(""),
                            ))
                })
                .collect();
            for session in sessions {
                let mut actor = session_actor(&state, session);
                if actor["role_ref"].is_null() {
                    actor["role_ref"] = step["executor"]["role_ref"].clone();
                }
                let key = actor["agent_instance_id"]
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| format!("session:{}", session.id));
                if p["agent_instance_id"].as_str().is_some_and(|id| id != key) {
                    continue;
                }
                let group = groups
                    .entry(key.clone())
                    .or_insert_with(|| json!({"id":key,"actor":actor,"entries":[]}));
                for turn in state
                    .turns
                    .iter()
                    .filter(|turn| turn.session_id == session.id)
                {
                    let task = state.governance.iter().find(|record| {
                        [
                            "configured_execution_task",
                            "archived_configured_execution_task",
                        ]
                        .contains(&record.r#type.as_str())
                            && record.definition_id == turn.id
                            && record.scope.project_id() == Some(scope.project.as_str())
                    });
                    // A session can continue across rounds. The frozen task is
                    // authoritative about which execution owns this exact Turn.
                    if task.is_some_and(|task| {
                        task.data["source"]["execution_id"] != entry.execution["id"]
                    }) || !seen_turns
                        .insert((entry.execution["id"].to_string(), turn.id.clone()))
                    {
                        continue;
                    }
                    let mut metadata = execution_metadata(&state, &scope, &entry)?;
                    metadata["outputs"] = json!(
                        metadata["outputs"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter(|output| {
                                output["turn_id"] == turn.id
                                    || (output["turn_id"].is_null()
                                        && entry.execution["turn_id"] == turn.id)
                            })
                            .cloned()
                            .collect::<Vec<_>>()
                    );
                    metadata.as_object_mut().unwrap().extend(json!({"kind":"step","execution_id":entry.execution["id"],"session_id":session.id,"snapshot_id":session.snapshot_id,"turn_id":turn.id,"status":turn.status,"actor":actor,"historical":entry.historical || entry.execution["turn_id"] != turn.id,"task_available":task.is_some(),"archived":task.is_some_and(|task|task.r#type=="archived_configured_execution_task"),"started":turn.started}).as_object().unwrap().clone());
                    group["entries"].as_array_mut().unwrap().push(metadata);
                }
            }
        }
        for controller in state.governance.iter().filter(|row| {
            row.r#type == "workflow_controller"
                && row.scope.project_id() == Some(scope.project.as_str())
                && row.data["root_instance_id"]
                    .as_str()
                    .is_some_and(|id| scope.instances.contains(id))
        }) {
            let Some(session) = state.sessions.iter().find(|session| {
                Some(session.id.as_str()) == controller.data["session_id"].as_str()
                    && session.project_id.as_deref() == Some(scope.project.as_str())
            }) else {
                continue;
            };
            if p["instance_id"]
                .as_str()
                .is_some_and(|id| controller.data["root_instance_id"] != id)
            {
                continue;
            }
            let actor = session_actor(&state, session);
            let key = actor["agent_instance_id"]
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| format!("session:{}", session.id));
            if p["agent_instance_id"].as_str().is_some_and(|id| id != key) {
                continue;
            }
            let group = groups
                .entry(key.clone())
                .or_insert_with(|| json!({"id":key,"actor":actor,"entries":[]}));
            let latest_turn = state
                .turns
                .iter()
                .rev()
                .find(|turn| turn.session_id == session.id)
                .map(|turn| turn.id.as_str());
            for turn in state
                .turns
                .iter()
                .filter(|turn| turn.session_id == session.id)
            {
                group["entries"].as_array_mut().unwrap().push(json!({"kind":"controller","instance_id":controller.data["root_instance_id"],"instance_name":controller.data["root_instance_id"].as_str().and_then(|id|state.configuration(id).ok()).map(|row|&row.name),"owner_kind":scope.owners.get(&session.owner_id).map(|owner|owner.0),"feature_id":session.feature_id,"step_name":"编排控制","owner_id":session.owner_id,"round":0,"session_id":session.id,"snapshot_id":session.snapshot_id,"turn_id":turn.id,"status":turn.status,"actor":actor,"historical":!controller.data["root_instance_id"].as_str().is_some_and(|id|scope.current_instances.contains(id)) || Some(turn.id.as_str()) != latest_turn,"task_available":false,"started":turn.started}));
            }
        }
        for group in groups.values_mut() {
            group["entries"].as_array_mut().unwrap().sort_by(|a, b| {
                a["historical"]
                    .as_bool()
                    .cmp(&b["historical"].as_bool())
                    .then_with(|| b["started"].as_u64().cmp(&a["started"].as_u64()))
                    .then_with(|| a["turn_id"].as_str().cmp(&b["turn_id"].as_str()))
            });
        }
        if let Some(snapshot_id) = p["snapshot_id"].as_str() {
            let session_id = text(p, "session_id")?;
            let allowed = groups.values().any(|group| {
                group["entries"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .any(|entry| {
                        entry["session_id"] == session_id && entry["snapshot_id"] == snapshot_id
                    })
            });
            require(allowed, "快照不属于当前 Agent 上下文")?;
            let snapshot = state
                .snapshots
                .iter()
                .find(|snapshot| snapshot.id == snapshot_id)
                .ok_or_else(|| err("not_found", "会话快照不可用"))?;
            return Ok(json!({"snapshot":snapshot}));
        }
        Ok(
            json!({"agents":groups.into_values().filter(|group|group["entries"].as_array().is_some_and(|entries|!entries.is_empty())).collect::<Vec<_>>(),"executions":executions }),
        )
    }
}

pub(crate) fn execution_actor(
    state: &EngineeringState,
    execution: &Value,
    step: &Value,
    definitions: &Value,
) -> Value {
    if let Some(session) = state
        .sessions
        .iter()
        .find(|session| Some(session.id.as_str()) == execution["session_id"].as_str())
    {
        let mut actor = session_actor(state, session);
        actor["kind"] = json!("agent");
        return actor;
    }
    let kind = step["executor"]["type"].as_str().unwrap_or("unknown");
    let tool_ref = step["executor"]["tool_ref"].as_str().unwrap_or("");
    let name = definitions[tool_ref]["name"]
        .as_str()
        .unwrap_or(match kind {
            "agent" => "尚无实际 Agent 会话",
            "workflow" => "子流程",
            "control" => "流程控制",
            "tool" => tool_ref,
            _ => "未记录执行方式",
        });
    json!({"kind":kind,"name":name,"assigned_agent_ref":execution["agent_revision_id"],"role_ref":step["executor"]["role_ref"]})
}
fn execution_metadata(
    state: &EngineeringState,
    scope: &WorkflowReadScope,
    entry: &ReadExecution,
) -> Result<Value> {
    let (plan, step) = frozen_step(state, entry)?;
    let e = entry.execution;
    let context_available = e["started"].is_number()
        || ["session_id", "turn_id", "tool_call_id"]
            .iter()
            .any(|key| e[key].is_string())
        || e["outputs"]
            .as_object()
            .is_some_and(|outputs| !outputs.is_empty())
        || state.governance.iter().any(|record| {
            [
                "configured_execution_task",
                "archived_configured_execution_task",
            ]
            .contains(&record.r#type.as_str())
                && record.scope == entry.instance.scope
                && record.data["source"]["execution_id"] == e["id"]
        });
    let execution_available = context_available
        || e["finished"].is_number()
        || state
            .governance
            .iter()
            .any(|record| record.scope == entry.instance.scope && execution_related(record, entry));
    let outputs: Vec<_> = e["outputs"]
        .as_object()
        .into_iter()
        .flatten()
        .map(|(port, id)| {
            let reference = step["outputs"][port]["delivery_ref"].as_str().unwrap_or("");
            let turn_id = id.as_str().and_then(|id|state.configuration(id).ok()).and_then(|output|output.data["provenance"]["turn_id"].as_str());
            json!({"id":id,"port":port,"name":plan["definitions"][reference]["name"],"turn_id":turn_id})
        })
        .collect();
    Ok(
        json!({"context_available":context_available,"execution_available":execution_available,"id":e["id"],"instance_id":entry.instance.id,"instance_name":entry.instance.name,"parent_execution_id":entry.instance.data["parent_execution_id"],"owner_id":entry.instance.data["owner_id"],"owner_kind":entry.instance.data["owner_id"].as_str().and_then(|id|scope.owners.get(id)).map(|owner|owner.0),"feature_id":entry.instance.data["owner_id"].as_str().and_then(|id|state.tasks.iter().find(|task|task.id==id).map(|task|task.feature_id.as_str()).or_else(||state.features.iter().find(|feature|feature.id==id).map(|feature|feature.id.as_str()))),"owner_title":entry.instance.data["owner_id"].as_str().and_then(|id|scope.owners.get(id)).map(|owner|&owner.1),"step":e["step"],"node_id":e["node_id"],"plan_revision_id":e["plan_revision_id"],"step_name":step["name"],"stage":step["stage"],"round":e["round"],"status":e["status"],"reason":e["reason"],"started":e["started"],"finished":e["finished"],"historical":entry.historical,"session_id":e["session_id"],"turn_id":e["turn_id"],"actor":execution_actor(state,e,step,&plan["definitions"]),"outputs":outputs}),
    )
}
fn execution_related(record: &ConfigurationRecord, entry: &ReadExecution) -> bool {
    let e = entry.execution;
    match record.r#type.as_str() {
        "workflow_decision" => {
            record.data["instance_id"] == entry.instance.id
                && record.data["execution_id"] == e["id"]
        }
        "workflow_confirmation" | "workflow_dispatch" | "workflow_intent" => {
            Some(record.definition_id.as_str()) == e["id"].as_str()
        }
        "platform_call" | "tool_call" => {
            Some(record.id.as_str()) == e["tool_call_id"].as_str()
                || (record.data["request"]["instance_id"] == entry.instance.id
                    && record.data["request"]["caller"]["execution_id"] == e["id"])
        }
        _ => false,
    }
}
impl Engine {
    pub(crate) fn workflow_executions(&self, p: &Value) -> Result<Value> {
        let state = self.store.snapshot()?;
        let scope = workflow_read_scope(&state, p)?;
        let entries: Vec<_> = read_executions(&state, &scope)
            .into_iter()
            .filter(|entry| {
                p["instance_id"]
                    .as_str()
                    .is_none_or(|id| entry.instance.id == id)
            })
            .collect();
        if let Some(id) = p["execution_id"].as_str() {
            let entry = entries
                .iter()
                .find(|entry| entry.execution["id"] == id)
                .ok_or_else(|| err("not_found", "执行不属于当前工作范围或尚未记录"))?;
            let records:Vec<_>=state.governance.iter().filter(|record|record.scope.project_id()==Some(scope.project.as_str()) && execution_related(record,entry)).map(|record|json!({"id":record.id,"type":record.r#type,"name":record.name,"created_at":record.created_at,"data":if ["platform_call","tool_call"].contains(&record.r#type.as_str()){json!({"status":record.data["status"],"receipt":record.data["receipt"],"error":record.data["error"]})} else {record.data.clone()}})).collect();
            return Ok(
                json!({"record":execution_metadata(&state,&scope,entry)?,"execution":entry.execution,"facts":records}),
            );
        }
        let mut records = entries
            .iter()
            .map(|entry| execution_metadata(&state, &scope, entry))
            .collect::<Result<Vec<_>>>()?;
        records.sort_by(|a, b| {
            a["historical"]
                .as_bool()
                .cmp(&b["historical"].as_bool())
                .then_with(|| a["instance_id"].as_str().cmp(&b["instance_id"].as_str()))
                .then_with(|| a["started"].as_u64().cmp(&b["started"].as_u64()))
        });
        Ok(json!({"records":records}))
    }
    pub(crate) fn workflow_record(&self, p: &Value) -> Result<Value> {
        let state = self.store.snapshot()?;
        let scope = workflow_read_scope(&state, p)?;
        let record = state.configuration(&text(p, "record_id")?)?;
        require(
            record.scope.project_id() == Some(scope.project.as_str()),
            "记录跨项目",
        )?;
        let allowed = match record.r#type.as_str() {
            "workflow_plan_revision" => record.data["instance_id"]
                .as_str()
                .is_some_and(|id| scope.instances.contains(id)),
            "workflow_change" => scope.instances.contains(&record.definition_id),
            _ => read_executions(&state, &scope)
                .iter()
                .any(|entry| execution_related(record, entry)),
        };
        require(allowed, "记录不属于当前流程")?;
        if ["platform_call", "tool_call"].contains(&record.r#type.as_str()) {
            // Match the public execution reader: expose receipts, never raw runtime/permission requests.
            return Ok(
                json!({"representation":"public_execution_receipt","record":{"id":record.id,"type":record.r#type,"name":record.name,"created_at":record.created_at,"data":{"status":record.data["status"],"receipt":record.data["receipt"],"error":record.data["error"]}}}),
            );
        }
        Ok(json!({"record":record}))
    }
}
