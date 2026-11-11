//! Storage codecs contain references and actual choices, never expanded source definitions.
use super::*;
use std::collections::{BTreeMap, BTreeSet};
const SOURCE_TEXT: &str = "\u{1e}devdesk-source-text:";

fn factual_field(key: &str) -> bool {
    [
        "inputs",
        "outputs",
        "arguments",
        "structured_content",
        "content",
        "value",
        "error",
        "input_sources",
    ]
    .contains(&key)
}

fn source_closure(state: &EngineeringState, value: &Value, ids: &mut BTreeSet<String>) {
    match value {
        Value::String(id)
            if state
                .configuration(id)
                .is_ok_and(|r| source_store::is_source(r)) =>
        {
            if ids.insert(id.clone()) {
                source_closure(state, &state.configuration(id).unwrap().data, ids);
            }
        }
        Value::Array(items) => {
            for item in items {
                source_closure(state, item, ids)
            }
        }
        Value::Object(items) => {
            for (key, item) in items {
                source_closure(state, &json!(key), ids);
                source_closure(state, item, ids)
            }
        }
        _ => {}
    }
}

pub(super) fn pack(state: &EngineeringState, value: &Value) -> Result<Value> {
    if let Value::String(text) = value {
        if text.starts_with(SOURCE_TEXT) {
            return Ok(json!(format!("{SOURCE_TEXT}{}", json!({"literal":text}))));
        }
        if let Ok(parsed) = serde_json::from_str::<Value>(text) {
            if parsed.to_string() == *text {
                let encoded = pack(state, &parsed)?;
                if encoded != parsed && restore(state, &encoded)? == parsed {
                    return Ok(json!(format!("{SOURCE_TEXT}{}", encoded)));
                }
            }
        }
        return Ok(value.clone());
    }
    if value.is_object() {
        if let Some(encoded) = pack_reference(state, value)? {
            return Ok(encoded);
        }
        let mut object = serde_json::Map::new();
        for (key, item) in value.as_object().unwrap() {
            object.insert(
                key.clone(),
                if factual_field(key) {
                    item.clone()
                } else if key == "policy" && value["renderer_version"] == ORCHESTRATION_RENDERER {
                    let context = value["controller_id"]
                        .as_str()
                        .and_then(|id| state.configuration(id).ok())
                        .and_then(|r| r.data["permission_context_ref"].as_str())
                        .and_then(|id| state.configuration(id).ok());
                    if let Some(context) = context.filter(|r| r.data["control_policy"] == *item) {
                        json!({"$source":"control_policy","context_ref":context.id})
                    } else {
                        pack(state, item)?
                    }
                } else {
                    pack(state, item)?
                },
            );
        }
        return Ok(Value::Object(object));
    }
    if let Value::Array(items) = value {
        return items
            .iter()
            .map(|v| pack(state, v))
            .collect::<Result<Vec<_>>>()
            .map(Value::Array);
    }
    Ok(value.clone())
}

fn bound_permission(state: &EngineeringState, id: &str) -> Result<BoundPermission> {
    let row = state.configuration(id)?;
    let binding: VersionedPermissionBinding = row.decode()?;
    resolve_bound_permission(
        state,
        binding.target.clone(),
        &PermissionSelection::Pinned {
            binding_revision_id: id.into(),
        },
    )
}

fn pack_reference(state: &EngineeringState, value: &Value) -> Result<Option<Value>> {
    if value.get("$source").is_some() {
        return Ok(Some(json!({"$source":"literal","value":value})));
    }
    if value.get("identity").is_some()
        && value.get("revision").is_some()
        && value.get("kind").is_some()
    {
        if let Ok(source) = serde_json::from_value::<SourceDefinition>(value.clone()) {
            let reference = source_reference(&source.identity, source.revision)?;
            if state
                .configuration(&reference)
                .is_ok_and(|r| r.source() == source)
            {
                return Ok(Some(json!({"$source":"source_definition","ref":reference})));
            }
        }
    }
    if value["renderer_version"] == "agent-task/v6"
        && value["source"]["workflow_instance_id"]
            .as_str()
            .is_some_and(|id| {
                state
                    .configuration(id)
                    .is_ok_and(|r| r.kind == "workflow_instance")
            })
    {
        let selection = json!({"$source":"task","source":value["source"],"inputs":value["inputs"],"input_sources":value["input_sources"],
                "agent_ref":value["agent"]["id"],"supplemental_input":value["supplemental_input"]});
        if restore(state, &selection).is_ok_and(|restored| restored == *value) {
            return Ok(Some(selection));
        }
        return Ok(Some(value.clone()));
    }
    if value.get("epoch").is_some()
        && value.get("policy_digest").is_some()
        && value.get("permission_context_ref").is_some()
    {
        let mut stored = serde_json::Map::new();
        for (key, item) in value.as_object().unwrap() {
            if !["policy", "policy_digest"].contains(&key.as_str()) {
                stored.insert(key.clone(), pack(state, item)?);
            }
        }
        stored.insert(
            "policy".into(),
            json!({"$source":"control_policy","context_ref":value["permission_context_ref"]}),
        );
        return Ok(Some(Value::Object(stored)));
    }
    if value.get("installation").is_some()
        && value.get("configuration_digest").is_some()
        && value["agent_configuration"].is_object()
    {
        return Ok(Some(
            json!({"$source":"runtime_binding","installation":value["installation"],"adapter_version":value["adapter_version"],
                "agent_configuration":pack(state,&value["agent_configuration"])?,"permissions":pack(state,&value["permissions"])?,
                "work_type":value["work_type"],"credential_ref":value["credential_ref"]}),
        ));
    }
    if value.get("agent_revision_id").is_some() && value.get("composition_hash").is_some() {
        return Ok(Some(
            json!({"$source":"agent_execution","agent_ref":value["agent_revision_id"],"instance_ref":value["agent_instance_id"],
                "integration_lock":pack(state,&value["integration_lock"])?}),
        ));
    }
    if value["binding_lock"].is_object()
        && value.get("enforcement").is_some()
        && value.get("scope").is_some()
    {
        return Ok(Some(
            json!({"$source":"execution_permissions","harness_binding_ref":value["binding_lock"]["harness"]["binding_revision_id"],
                "sandbox_binding_ref":value["binding_lock"]["sandbox"]["binding_revision_id"],"scope":value["scope"]}),
        ));
    }
    if value.get("files").is_some()
        && value.get("requires_tools").is_some()
        && value.get("body").is_some()
    {
        if let Some(package) = state
            .governance
            .iter()
            .find(|r| r.kind == "skill_package" && r.data == *value)
        {
            return Ok(Some(json!({"$source":"skill_package","ref":package.id})));
        }
    }
    if value.get("workflow_revision_id").is_some() && value.get("specification_view").is_some() {
        let plan: ConfiguredWorkflowPlan =
            serde_json::from_value(value.clone()).map_err(storage)?;
        let mut bindings = BTreeMap::new();
        let mut permissions = BTreeMap::new();
        let mut connections = BTreeMap::new();
        for (agent, lock) in &plan.integration_locks {
            let composition = state.resolve_agent(agent)?;
            bindings.insert(composition.role_ref.unwrap(), agent.clone());
            for tool in &lock.tools {
                connections.insert(tool.revision_id.clone(), tool.binding_ref.clone());
            }
        }
        for (key, lock) in &plan.permission_locks {
            permissions.insert(key.clone(),json!({"harness":lock.harness.binding_revision_id,"sandbox":lock.sandbox.binding_revision_id}));
        }
        for (tool, bound) in &plan.tool_bindings {
            connections.insert(tool.clone(), bound.binding_ref.clone());
        }
        // The consumer owns this execution selection. A store may contain multiple
        // projects; the first catalog entry is unrelated to this particular plan.
        let project_id = plan
            .work_configuration_ref
            .as_deref()
            .and_then(|id| state.configuration(id).ok())
            .and_then(|r| r.scope.project())
            .or_else(|| {
                state
                    .governance
                    .iter()
                    .find(|r| r.kind == "workflow_plan_revision" && r.data["plan"] == *value)
                    .and_then(|r| r.scope.project())
            })
            .or_else(|| {
                state
                    .configuration(&plan.workflow_revision_id)
                    .ok()
                    .and_then(|r| r.scope.project())
            })
            .or_else(|| (state.projects.len() == 1).then(|| state.projects[0].id.as_str()));
        return Ok(Some(
            json!({"$source":"workflow_plan","workflow_ref":plan.workflow_revision_id,"standard_ref":plan.standard_ref,"work_configuration_ref":plan.work_configuration_ref,"role_agent_bindings":bindings,
                "permission_context_ref":plan.permission_context_ref,"permission_workflow_ref":plan.permission_workflow_ref,"permission_bindings":permissions,"integration_bindings":connections,
                "project_id":project_id}),
        ));
    }
    // Copies embedded in receipts/tasks are regenerated from their owning source.
    if value.get("definition_id").is_some() && value.get("content_hash").is_some() {
        if let Some(id) = value["id"].as_str() {
            if state
                .configuration(id)
                .is_ok_and(|r| source_store::is_source(r) && json!(r) == *value)
            {
                return Ok(Some(json!({"$source":"source","ref":id})));
            }
        }
    }
    if value.get("binding_revision_id").is_some()
        && value.get("profile_hash").is_some()
        && value.get("profile").is_some()
    {
        let reference =
            json!({"$source":"bound_permission","binding_ref":value["binding_revision_id"]});
        if restore(state, &reference).is_ok_and(|expected| expected == *value) {
            return Ok(Some(reference));
        }
    }
    if value.get("integration_hash").is_some()
        && value.get("binding_ref").is_some()
        && value.get("definition").is_some()
    {
        let reference = json!({"$source":"bound_tool","tool_ref":value["revision_id"],"binding_ref":value["binding_ref"]});
        if restore(state, &reference).is_ok_and(|expected| expected == *value) {
            return Ok(Some(reference));
        }
    }
    Ok(None)
}

pub(super) fn restore(state: &EngineeringState, value: &Value) -> Result<Value> {
    restore_with(state, value, &mut BTreeMap::new())
}
pub(super) fn restore_with(
    state: &EngineeringState,
    value: &Value,
    memo: &mut BTreeMap<String, Value>,
) -> Result<Value> {
    if let Value::String(text) = value {
        if let Some(encoded) = text.strip_prefix(SOURCE_TEXT) {
            let encoded: Value = serde_json::from_str(encoded).map_err(storage)?;
            if let Some(literal) = encoded["literal"].as_str() {
                return Ok(json!(literal));
            }
            return Ok(json!(restore_with(state, &encoded, memo)?.to_string()));
        }
    }
    if value["$source"].is_string() {
        return restore_reference(state, value, memo);
    }
    match value {
        Value::Object(items) => {
            let mut object = items
                .iter()
                .map(|(k, v)| {
                    Ok((
                        k.clone(),
                        if factual_field(k) {
                            v.clone()
                        } else {
                            restore_with(state, v, memo)?
                        },
                    ))
                })
                .collect::<Result<serde_json::Map<_, _>>>()?;
            if object.contains_key("epoch")
                && object.contains_key("permission_context_ref")
                && object.contains_key("policy")
            {
                object.insert(
                    "policy_digest".into(),
                    json!(configuration_hash(
                        &serde_json::from_value::<WorkflowControlPolicy>(object["policy"].clone())
                            .map_err(storage)?
                    )),
                );
            }
            Ok(Value::Object(object))
        }
        Value::Array(items) => items
            .iter()
            .map(|v| restore_with(state, v, memo))
            .collect::<Result<Vec<_>>>()
            .map(Value::Array),
        _ => Ok(value.clone()),
    }
}

// Keep the large typed reconstruction frame off the generic JSON recursion path.
fn restore_reference(
    state: &EngineeringState,
    value: &Value,
    memo: &mut BTreeMap<String, Value>,
) -> Result<Value> {
    match value["$source"].as_str() {
        Some("literal") => return Ok(value["value"].clone()),
        Some("source_definition") => {
            return Ok(json!(
                state
                    .configuration(value["ref"].as_str().unwrap_or(""))?
                    .source()
            ));
        }
        Some("task") => {
            let source = &value["source"];
            let instance_ref = source["workflow_instance_id"].as_str().unwrap_or("");
            let plan_record =
                state.configuration(source["plan_revision_id"].as_str().unwrap_or(""))?;
            let plan = restore_with(state, &plan_record.data["plan"], memo)?;
            let mut instance = state.configuration(instance_ref)?.data.clone();
            instance["plan"] = plan;
            instance["workflow_ref"] = source["workflow_revision_id"].clone();
            instance["active_plan_revision_id"] = source["plan_revision_id"].clone();
            instance["role_agent_bindings"] = plan_record.data["role_agent_bindings"].clone();
            let instance: ConfiguredWorkflowInstance =
                serde_json::from_value(instance).map_err(storage)?;
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
                        .find(|r| {
                            r.kind == "workflow_execution_history" && r.data["id"] == execution_ref
                        })
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
            return Ok(input);
        }
        Some("control_policy") => {
            let policy: WorkflowControlPolicy = serde_json::from_value(
                state
                    .configuration(value["context_ref"].as_str().unwrap_or(""))?
                    .data["control_policy"]
                    .clone(),
            )
            .map_err(storage)?;
            return Ok(json!(policy));
        }
        Some("runtime_binding") => {
            let configuration: AgentExecutionConfiguration =
                serde_json::from_value(restore_with(state, &value["agent_configuration"], memo)?)
                    .map_err(storage)?;
            let model: ModelDefinition = state
                .configuration(&configuration.model_revision_id)?
                .decode()?;
            let harness: HarnessDefinition = state
                .configuration(&configuration.harness_revision_id)?
                .decode()?;
            let connection = resolve_model_connection(&model, &harness)?;
            let profile_digest = connection
                .as_ref()
                .map(|connection| {
                    model_providers::dsh_model_patch(connection, &model.model)
                        .and_then(|v| configuration_digest(&json!(v)))
                })
                .transpose()?;
            let permissions: Option<ExecutionPermissions> =
                serde_json::from_value(restore_with(state, &value["permissions"], memo)?)
                    .map_err(storage)?;
            let writable = permissions
                .as_ref()
                .map(|p| p.scope.writable_paths.clone())
                .unwrap_or_default();
            let readonly = permissions
                .as_ref()
                .map(|p| p.scope.read_only_paths.clone())
                .unwrap_or_default();
            let policy = if writable.is_empty() {
                ToolPolicy::ReadOnly
            } else if writable == ["."] {
                ToolPolicy::WorkspaceWrite
            } else {
                ToolPolicy::ScopedWrite
            };
            let mut binding = RuntimeBinding {
                model_connection: connection,
                model_profile_digest: profile_digest,
                agent_configuration: Some(configuration),
                permissions,
                installation: serde_json::from_value(value["installation"].clone())
                    .map_err(storage)?,
                adapter_version: value["adapter_version"].as_str().unwrap_or("").into(),
                model: model.model,
                model_parameters: Default::default(),
                tool_policy: policy,
                writable_paths: writable,
                read_only_paths: readonly,
                work_type: serde_json::from_value(value["work_type"].clone()).map_err(storage)?,
                configuration_digest: String::new(),
                credential_ref: value["credential_ref"].as_str().map(str::to_owned),
            };
            let agent = state
                .definitions
                .iter()
                .find(|a| {
                    a.id == binding
                        .agent_configuration
                        .as_ref()
                        .unwrap()
                        .agent_revision_id
                })
                .ok_or_else(|| err("storage", "Agent 源视图缺失"))?;
            binding.configuration_digest = runtime_digest(agent, &binding)?;
            return Ok(json!(binding));
        }
        Some("agent_execution") => {
            let reference = value["agent_ref"].as_str().unwrap_or("");
            let agent = state.resolve_agent(reference)?;
            let model: ModelDefinition = state.configuration(&agent.model_ref)?.decode()?;
            let lock = restore_with(state, &value["integration_lock"], memo)?;
            return Ok(json!(AgentExecutionConfiguration {
                agent_instance_id: value["instance_ref"].as_str().unwrap_or("").into(),
                agent_revision_id: reference.into(),
                composition_hash: state.configuration(reference)?.content_hash.clone(),
                model_revision_id: agent.model_ref,
                harness_revision_id: agent.harness_ref,
                model_provider: model.provider,
                model_parameters: model.parameters,
                tool_revisions: agent.tools,
                integration_lock: serde_json::from_value(lock).map_err(storage)?
            }));
        }
        Some("execution_permissions") => {
            let lock = VersionedPermissionLock {
                harness: bound_permission(
                    state,
                    value["harness_binding_ref"].as_str().unwrap_or(""),
                )?,
                sandbox: bound_permission(
                    state,
                    value["sandbox_binding_ref"].as_str().unwrap_or(""),
                )?,
            };
            let scope = serde_json::from_value(value["scope"].clone()).map_err(storage)?;
            return Ok(json!(permission_profiles::compile_permission_profiles(
                state,
                &lock,
                Some(scope)
            )?));
        }
        Some("skill_package") => {
            return Ok(state
                .configuration(value["ref"].as_str().unwrap_or(""))?
                .data
                .clone());
        }
        Some("source") => {
            return Ok(json!(
                state.configuration(value["ref"].as_str().unwrap_or(""))?
            ));
        }
        Some("bound_permission") => {
            return Ok(json!(bound_permission(
                state,
                value["binding_ref"].as_str().unwrap_or("")
            )?));
        }
        Some("bound_tool") => {
            let tool = state.configuration(value["tool_ref"].as_str().unwrap_or(""))?;
            let binding = state.configuration(value["binding_ref"].as_str().unwrap_or(""))?;
            let definition: ExternalTool = tool.decode()?;
            let integration = state.configuration(&definition.integration_ref)?;
            return Ok(json!(BoundTool {
                revision_id: tool.id.clone(),
                content_hash: tool.content_hash.clone(),
                definition,
                integration: integration.decode()?,
                integration_hash: integration.content_hash.clone(),
                binding_ref: binding.id.clone(),
                binding: binding.decode()?,
                binding_hash: binding.content_hash.clone()
            }));
        }
        Some("workflow_plan") => {
            let cache_key = value.to_string();
            if let Some(plan) = memo.get(&cache_key) {
                return Ok(plan.clone());
            }
            let mut ids = BTreeSet::new();
            source_closure(state, value, &mut ids);
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
                    .filter(|r| r.kind == "skill_package" && ids.contains(&r.definition_id))
                    .cloned(),
            );
            let scope = value["project_id"]
                .as_str()
                .map(|id| Scope::Project {
                    project_id: id.into(),
                })
                .unwrap_or(Scope::Global);
            let mut policy = ConfigurationRecord {
                id: "resolver-context".into(),
                kind: "project_configuration".into(),
                definition_id: "resolver".into(),
                scope: scope.clone(),
                name: "execution source choices".into(),
                version: u64::MAX,
                supersedes: None,
                content_hash: String::new(),
                data: json!({"permission_context_ref":value["permission_context_ref"],"permission_workflow_ref":value["permission_workflow_ref"],"workflow_ref":value["workflow_ref"]}),
                immutable: true,
                revision: 1,
                created_at: 0,
            };
            policy.content_hash = configuration_hash(&policy.data);
            context.governance.push(policy);
            for choice in value["permission_bindings"]
                .as_object()
                .into_iter()
                .flat_map(|v| v.values())
            {
                for layer in ["harness", "sandbox"] {
                    let id = choice[layer]
                        .as_str()
                        .ok_or_else(|| err("storage", "权限选择缺失"))?;
                    let binding = state.configuration(id)?;
                    let data: VersionedPermissionBinding = binding.decode()?;
                    let mut row = binding.clone();
                    row.kind = "permission_assignment".into();
                    row.id = format!("selected:{id}");
                    row.version = u64::MAX;
                    row.data = json!({"target":data.target,"binding_revision_id":id});
                    row.content_hash = configuration_hash(&row.data);
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
            if let Some(project) = scope.project() {
                let mut row = context
                    .governance
                    .last()
                    .cloned()
                    .ok_or_else(|| err("storage", "源选择缺失"))?;
                row.id = "selected-integrations".into();
                row.kind = "integration_project_configuration".into();
                row.definition_id = format!("project-integrations.{project}");
                row.version = u64::MAX;
                row.data = Value::Object(mapping);
                context.governance.push(row);
            }
            context.refresh_configuration_views()?;
            let bindings: BTreeMap<Id, Id> =
                serde_json::from_value(value["role_agent_bindings"].clone()).map_err(storage)?;
            let workflow = context.configuration(value["workflow_ref"].as_str().unwrap_or(""))?;
            let mut plan =
                compile_workflow_sources(None, &context, workflow, &scope, Some(&bindings))?;
            plan.standard_ref = value["standard_ref"].as_str().map(str::to_owned);
            plan.work_configuration_ref =
                value["work_configuration_ref"].as_str().map(str::to_owned);
            if let Some(reference) = &plan.standard_ref {
                standard_snapshot(state, reference, &scope)?;
            }
            plan.permission_workflow_ref =
                value["permission_workflow_ref"].as_str().map(str::to_owned);
            for (key, choice) in value["permission_bindings"]
                .as_object()
                .into_iter()
                .flatten()
            {
                plan.permission_locks.insert(
                    key.clone(),
                    VersionedPermissionLock {
                        harness: bound_permission(state, choice["harness"].as_str().unwrap_or(""))?,
                        sandbox: bound_permission(state, choice["sandbox"].as_str().unwrap_or(""))?,
                    },
                );
            }
            let plan = json!(plan);
            memo.insert(cache_key, plan.clone());
            return Ok(plan);
        }
        Some(_) => return Err(err("storage", "未知源视图类型")),
        None => return Err(err("storage", "源视图标记缺失")),
    }
}

pub fn runtime_digest(agent: &Definition, b: &RuntimeBinding) -> Result<String> {
    let mut value = json!({"installation":b.installation,"adapter":b.adapter_version,"agent":agent,"agent_configuration":b.agent_configuration,
        "model":b.model,"model_connection":b.model_connection,"model_profile_digest":b.model_profile_digest,"policy":b.tool_policy,"writable_paths":b.writable_paths,"read_only_paths":b.read_only_paths,"work_type":b.work_type,
        "sandbox":"docker-worker/v1","credential_ref":b.credential_ref,"permissions":b.permissions});
    if !b.model_parameters.is_empty() {
        value["model_parameters"] = json!(b.model_parameters);
    }
    configuration_digest(&value)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn source_records_are_references_but_forged_and_factual_values_are_preserved() {
        let mut state = EngineeringState::default();
        seed_configuration(&mut state);
        let record = state.governance.iter().find(|r| r.kind == "agent").unwrap();
        let original = json!(record);
        let packed = pack(&state, &original).unwrap();
        assert_eq!(packed["$source"], "source");
        assert_eq!(restore(&state, &packed).unwrap(), original);
        let mut forged = original.clone();
        forged["data"]["role_ref"] = json!("untrusted:1");
        assert_eq!(pack(&state, &forged).unwrap(), forged);
        let actual = json!({"arguments":{"$source":"source","ref":record.id},"inputs":{"nested":forged},"outputs":{"$source":"unrecognized-user-value"}});
        assert_eq!(
            restore(&state, &pack(&state, &actual).unwrap()).unwrap(),
            actual
        );
    }
    #[test]
    fn nested_material_metadata_round_trips_on_the_normal_worker_stack() {
        std::thread::Builder::new()
            .stack_size(2 * 1024 * 1024)
            .spawn(|| {
                let state = EngineeringState::default();
                let mut value = json!({"leaf":"actual metadata"});
                for _ in 0..48 {
                    value = json!({"metadata":[value]});
                }
                let original = json!({"record":value.to_string()});
                assert_eq!(
                    restore(&state, &pack(&state, &original).unwrap()).unwrap(),
                    original
                );
            })
            .unwrap()
            .join()
            .unwrap();
    }
    #[test]
    fn user_objects_cannot_be_confused_with_storage_references() {
        let state = EngineeringState::default();
        for original in [
            json!({"$source":"source","ref":"user-data:1"}),
            json!({"metadata":{"$source":"custom-format","value":[1,2]}}),
        ] {
            let packed = pack(&state, &original).unwrap();
            assert_eq!(restore(&state, &packed).unwrap(), original);
        }
    }
    #[test]
    fn literal_reserved_text_is_escaped_without_changing_user_input() {
        let state = EngineeringState::default();
        let original = json!(format!("{SOURCE_TEXT}{{\"$source\":\"user-text\"}}"));
        assert_eq!(
            restore(&state, &pack(&state, &original).unwrap()).unwrap(),
            original
        );
    }
}
