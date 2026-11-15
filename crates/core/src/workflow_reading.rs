//! Scoped, read-only navigation over existing frozen workflow facts.
use crate::*;
use std::collections::{BTreeMap, BTreeSet};

pub(crate) struct WorkflowReadScope {
    pub project: String,
    pub owners: BTreeMap<String, (&'static str, String)>,
    pub instances: BTreeSet<String>,
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
            owners.insert(id, ("project", row.title.clone()));
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
    Ok(WorkflowReadScope {
        project,
        owners,
        instances,
    })
}

struct ReadExecution<'a> {
    instance: &'a ConfigurationRecord,
    execution: &'a Value,
    historical: bool,
}
fn read_executions<'a>(
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
                    historical: false,
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
fn frozen_step<'a>(
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
        let mut groups: BTreeMap<String, Value> = BTreeMap::new();
        for entry in read_executions(&state, &scope) {
            if p["instance_id"]
                .as_str()
                .is_some_and(|id| entry.instance.id != id)
            {
                continue;
            }
            let (_, step) = frozen_step(&state, &entry)?;
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
                    group["entries"].as_array_mut().unwrap().push(json!({"kind":"step","instance_id":entry.instance.id,"instance_name":entry.instance.name,"execution_id":entry.execution["id"],"step_name":step["name"],"owner_id":session.owner_id,"owner_title":scope.owners.get(&session.owner_id).map(|owner|&owner.1),"round":entry.execution["round"],"session_id":session.id,"snapshot_id":session.snapshot_id,"turn_id":turn.id,"status":turn.status,"actor":actor,"historical":entry.historical || entry.execution["turn_id"] != turn.id,"task_available":task.is_some(),"archived":task.is_some_and(|task|task.r#type=="archived_configured_execution_task"),"started":turn.started}));
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
            for turn in state
                .turns
                .iter()
                .filter(|turn| turn.session_id == session.id)
            {
                group["entries"].as_array_mut().unwrap().push(json!({"kind":"controller","instance_id":controller.data["root_instance_id"],"step_name":"编排控制","owner_id":session.owner_id,"round":0,"session_id":session.id,"snapshot_id":session.snapshot_id,"turn_id":turn.id,"status":turn.status,"actor":actor,"historical":false,"task_available":false,"started":turn.started}));
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
        Ok(json!({"agents":groups.into_values().collect::<Vec<_>>() }))
    }
}
