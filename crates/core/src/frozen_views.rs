//! Pure reconstruction of frozen application selections; no environment access.
use crate::*;
use std::collections::{BTreeMap, BTreeSet};
fn storage(e: impl std::fmt::Display) -> Error {
    err("storage", e.to_string())
}
fn is_source(r: &ConfigurationRecord) -> bool {
    r.is_definition()
        || ["project_configuration", "integration_project_configuration"]
            .contains(&r.r#type.as_str())
}
fn source_closure(
    state: &EngineeringState,
    value: &Value,
    ids: &mut BTreeSet<String>,
) -> Result<()> {
    fn selected(value: &Value, seeds: &mut Vec<String>) {
        match value {
            Value::String(v) => {
                if parse_source_reference(v).is_ok() {
                    seeds.push(v.clone());
                }
            }
            Value::Array(values) => {
                for v in values {
                    selected(v, seeds)
                }
            }
            Value::Object(values) => {
                for (k, v) in values {
                    selected(&json!(k), seeds);
                    selected(v, seeds)
                }
            }
            _ => {}
        }
    }
    let mut seeds = Vec::new();
    for key in [
        "workflow_ref",
        "standard_ref",
        "harness_permission_ref",
        "execution_selection",
        "base_workflow_ref",
        "role_agent_bindings",
        "permission_bindings",
        "integration_bindings",
    ] {
        selected(&value[key], &mut seeds);
    }
    while let Some(id) = seeds.pop() {
        if ids.contains(&id) {
            continue;
        }
        let record = state.configuration(&id)?;
        if is_source(record) {
            ids.insert(id);
            for edge in configuration_edges(record)? {
                seeds.push(resolve_configuration_edge(state, &edge)?.id.clone());
            }
        }
    }
    Ok(())
}
pub fn selection_configuration(choice: &ExecutionPermissionSelection) -> Value {
    let profile = |selection: &PermissionSelection| match selection {
        PermissionSelection::Profile {
            permission_revision_id,
        } => Some(permission_revision_id.clone()),
        _ => None,
    };
    json!({"sandbox_ref":choice.sandbox_ref,"harness_permission_ref":profile(&choice.harness),"sandbox_permission_ref":profile(&choice.sandbox)})
}
pub fn pin_permission_defaults(
    state: &mut EngineeringState,
    locks: &BTreeMap<Id, VersionedPermissionLock>,
) -> Result<()> {
    for lock in locks.values() {
        for bound in [&lock.harness, &lock.sandbox] {
            if bound.binding_revision_id == bound.profile.id {
                continue;
            }
            let mut row = state.configuration(&bound.binding_revision_id)?.clone();
            row.r#type = "permission_assignment".into();
            row.id = format!("selected:{}", bound.binding_revision_id);
            row.version = u64::MAX;
            row.data = json!({"target":bound.binding.target,"binding_revision_id":bound.binding_revision_id});
            row.content_hash = content_digest(&row.refs, &row.data);
            state.governance.push(row);
        }
    }
    Ok(())
}
pub fn permission_choice(bound: &BoundPermission) -> Value {
    let selection = if bound.binding_revision_id == bound.profile.id {
        PermissionSelection::Profile {
            permission_revision_id: bound.profile.id.clone(),
        }
    } else {
        PermissionSelection::Pinned {
            binding_revision_id: bound.binding_revision_id.clone(),
        }
    };
    json!({"target":bound.binding.target,"selection":selection})
}
pub fn restore_permission_choice(
    state: &EngineeringState,
    value: &Value,
) -> Result<BoundPermission> {
    let target = serde_json::from_value(value["target"].clone()).map_err(storage)?;
    let selection = serde_json::from_value(value["selection"].clone()).map_err(storage)?;
    resolve_bound_permission(state, target, &selection)
}

pub fn restore_task(state: &EngineeringState, value: &Value, plan: Value) -> Result<Value> {
    let source = &value["source"];
    let plan_record = state.configuration(source["plan_revision_id"].as_str().unwrap_or(""))?;
    let instance_ref = source["workflow_instance_id"].as_str().unwrap_or("");

    let mut instance = state.configuration(instance_ref)?.data.clone();
    instance["plan"] = plan;
    instance["workflow_ref"] = source["workflow_revision_id"].clone();
    instance["active_plan_revision_id"] = source["plan_revision_id"].clone();
    instance["role_agent_bindings"] = plan_record.data["role_agent_bindings"].clone();
    instance["human_governance"] = json!(plan_record.data["human_governance"] == true);
    if let Some(request) = plan_record.data.get("user_request") {
        instance["user_request"] = request.clone();
    }
    let instance: ConfiguredWorkflowInstance = serde_json::from_value(instance).map_err(storage)?;
    let execution_ref = source["execution_id"].as_str().unwrap_or("");
    let mut execution = instance
        .steps
        .iter()
        .find(|e| e.id == execution_ref)
        .cloned()
        .or_else(|| {
            state
                .governance
                .iter()
                .find(|r| r.r#type == "workflow_execution_history" && r.data["id"] == execution_ref)
                .and_then(|r| serde_json::from_value(r.data.clone()).ok())
        })
        .ok_or_else(|| err("storage", "任务执行来源缺失"))?;
    execution.inputs = serde_json::from_value(value["inputs"].clone()).map_err(storage)?;
    execution.agent_revision_id = value["agent_ref"].as_str().map(str::to_owned);
    let step = instance
        .plan
        .document
        .steps
        .iter()
        .find(|s| s.key == source["step"])
        .ok_or_else(|| err("storage", "任务源步骤缺失"))?;
    let input = render_source_task(
        &instance,
        step,
        &execution,
        value["supplemental_input"].as_str(),
    )?;
    let mut input: Value = serde_json::from_str(&input).map_err(storage)?;
    input["source"] = source.clone();
    input["input_sources"] = value["input_sources"].clone();
    Ok(input)
}
pub fn restore_plan(
    state: &EngineeringState,
    value: &Value,
    memo: &mut BTreeMap<String, Value>,
) -> Result<Value> {
    let cache_key = value.to_string();
    if let Some(plan) = memo.get(&cache_key) {
        return Ok(plan.clone());
    }
    let mut ids = BTreeSet::new();
    source_closure(state, value, &mut ids)?;
    let mut context = EngineeringState {
        governance: state
            .governance
            .iter()
            .filter(|r| ids.contains(&r.id))
            .cloned()
            .collect(),
        projects: state.projects.clone(),
        ..Default::default()
    };
    context.governance.extend(
        state
            .governance
            .iter()
            .filter(|r| r.r#type == "skill_package" && ids.contains(&r.definition_id))
            .cloned(),
    );
    let scope = value["project_id"]
        .as_str()
        .map(Scope::project)
        .unwrap_or(Scope::global());
    let choice: ExecutionPermissionSelection =
        serde_json::from_value(value["execution_selection"].clone()).map_err(storage)?;
    let mut selected = selection_configuration(&choice);
    selected["workflow_ref"] = value["workflow_ref"].clone();
    selected["base_workflow_ref"] = value["base_workflow_ref"].clone();
    let mut policy = ConfigurationRecord {
        id: "resolver-context".into(),
        r#type: "project_configuration".into(),
        definition_id: "resolver".into(),
        scope: scope.clone(),
        name: "execution source choices".into(),
        version: u64::MAX,
        supersedes: None,
        content_hash: String::new(),
        refs: References::new(),
        data: selected,
        immutable: true,
        revision: 1,
        created_at: 0,
    };
    policy.content_hash = content_digest(&policy.refs, &policy.data);
    context.governance.push(policy);
    for choice in value["permission_bindings"]
        .as_object()
        .into_iter()
        .flat_map(|v| v.values())
    {
        for layer in ["harness", "sandbox"] {
            let bound = restore_permission_choice(state, &choice[layer])?;
            if bound.binding_revision_id == bound.profile.id {
                continue;
            }
            let id = bound.binding_revision_id.as_str();
            let binding = state.configuration(id)?;
            let data = bound.binding;
            let mut row = binding.clone();
            row.r#type = "permission_assignment".into();
            row.id = format!("selected:{id}");
            row.version = u64::MAX;
            row.data = json!({"target":data.target,"binding_revision_id":id});
            row.content_hash = content_digest(&row.refs, &row.data);
            context.governance.push(row);
        }
    }
    let mut mapping = serde_json::Map::new();
    for (tool, binding) in value["integration_bindings"]
        .as_object()
        .into_iter()
        .flatten()
    {
        let definition: ExternalTool = state.configuration(tool)?.decode()?;
        mapping.insert(definition.integration_ref, json!(binding));
    }
    if let Some(project) = scope.project_id() {
        let mut row = context
            .governance
            .last()
            .cloned()
            .ok_or_else(|| err("storage", "源选择缺失"))?;
        row.id = "selected-integrations".into();
        row.r#type = "integration_project_configuration".into();
        row.scope = scope.clone();
        row.definition_id = format!("project-integrations.{project}");
        row.version = u64::MAX;
        row.data = Value::Object(mapping);
        row.content_hash = content_digest(&row.refs, &row.data);
        context.governance.push(row);
    }
    context.refresh_configuration_views()?;
    let bindings: BTreeMap<Id, Id> =
        serde_json::from_value(value["role_agent_bindings"].clone()).map_err(storage)?;
    let workflow = context.configuration(value["workflow_ref"].as_str().unwrap_or(""))?;
    let mut plan = compile_workflow_sources(None, &context, workflow, &scope, Some(&bindings))?;
    plan.harness_permission_ref = value["harness_permission_ref"].as_str().map(str::to_owned);
    plan.standard_ref = value["standard_ref"].as_str().map(str::to_owned);
    plan.work_configuration_ref = value["work_configuration_ref"].as_str().map(str::to_owned);
    if let Some(reference) = &plan.standard_ref {
        standard_snapshot(state, reference, &scope)?;
    }
    plan.base_workflow_ref = value["base_workflow_ref"].as_str().map(str::to_owned);
    for (key, choice) in value["permission_bindings"]
        .as_object()
        .into_iter()
        .flatten()
    {
        plan.permission_locks.insert(
            key.clone(),
            VersionedPermissionLock {
                harness: restore_permission_choice(state, &choice["harness"])?,
                sandbox: restore_permission_choice(state, &choice["sandbox"])?,
            },
        );
    }
    let plan = json!(plan);
    memo.insert(cache_key, plan.clone());
    Ok(plan)
}
