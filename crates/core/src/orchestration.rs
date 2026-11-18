//! Frozen plan publication and bounded control reads.
use crate::{governance::record, *};
/// Only configuration is read or published here; the actual instance is an
/// explicit argument and the immutable runtime plan is returned to its caller.
pub(crate) fn freeze_plan_record(
    state: &mut EngineeringState,
    instance: &mut ConfiguredWorkflowInstance,
    scope: &Scope,
    reason: &str,
) -> Result<ConfigurationRecord> {
    instance.human_governance |= instance
        .plan
        .document
        .steps
        .iter()
        .any(|step| step.confirmation == ConfirmationPolicy::Human);
    instance
        .plan
        .document
        .assign_node_ids(&instance.workflow_ref);
    // A changed plan is a new source revision, never a persisted compiled view.
    let mut source_document = instance.plan.document.clone();
    for step in &mut source_document.steps {
        step.access.writable_paths.clear();
        step.access.read_only_paths.clear();
        step.access.tool_refs.clear();
    }
    let current_source = state
        .configuration(&instance.plan.workflow_revision_id)?
        .clone();
    let mut original: WorkflowDefinition = current_source.decode()?;
    original.assign_node_ids(&current_source.id);
    for step in &mut original.steps {
        step.access.writable_paths.clear();
        step.access.read_only_paths.clear();
        step.access.tool_refs.clear();
    }
    if current_source.r#type != "workflow"
        || serde_json::to_value(&source_document).unwrap()
            != serde_json::to_value(&original).unwrap()
    {
        let data = json!(source_document);
        let source = record(
            "workflow",
            format!(
                "execution-workflow.{}",
                configuration_hash(
                    &json!({"scope":crate::standards::source_scope(scope),"content":canonical_standard_content(&data)})
                )
            ),
            crate::standards::source_scope(scope),
            reason.into(),
            1,
            None,
            data,
            true,
        );
        instance.plan.workflow_revision_id = source.id.clone();
        instance.workflow_ref = source.id.clone();
        instance
            .plan
            .definitions
            .insert(source.id.clone(), source.clone());
        if state.configuration(&source.id).is_err() {
            state.governance.push(source);
        }
    }
    let existing = instance.plan.standard_ref.clone();
    let included = existing
        .as_deref()
        .and_then(|r| standard_snapshot(state, r, scope).ok())
        .is_some_and(|s| {
            s.nodes
                .iter()
                .any(|n| n.kind == "workflow" && n.reference == instance.plan.workflow_revision_id)
        });
    if !included {
        let mut roots = existing
            .as_deref()
            .and_then(|r| state.configuration(r).ok())
            .and_then(|r| reference_list(&r.refs, "root_workflow_refs").ok())
            .unwrap_or_default();
        if let Some(old) = instance
            .plan
            .base_workflow_ref
            .as_deref()
            .and_then(|old| roots.iter().position(|r| r == old))
        {
            roots[old] = instance.plan.workflow_revision_id.clone();
        } else {
            roots = vec![instance.plan.workflow_revision_id.clone()];
        }
        let source = crate::standards::capture_standard(state, roots, scope, reason)?;
        instance.plan.standard_ref = Some(source.id.clone());
        instance.plan.definitions.insert(source.id.clone(), source);
    }
    let plan = record(
        "workflow_plan_revision",
        instance.root_instance_id.clone(),
        scope.clone(),
        reason.into(),
        1,
        None,
        json!({"owner_id":instance.owner_id,"user_request":instance.user_request,"human_governance":instance.human_governance,"standard_ref":instance.plan.standard_ref,"workflow_ref":instance.plan.workflow_revision_id,"work_configuration_ref":instance.plan.work_configuration_ref,"instance_id":instance.steps.first().map(|e|&e.workflow_instance_id),"plan":instance.plan,"inputs":instance.inputs,"role_agent_bindings":instance.role_agent_bindings,
            "parent_revision_id":if instance.active_plan_revision_id.is_empty(){None}else{Some(&instance.active_plan_revision_id)},"reason":reason}),
        true,
    );
    instance.active_plan_revision_id = plan.id.clone();
    for execution in &mut instance.steps {
        if execution.plan_revision_id.is_empty()
            && let Some(step) = instance
                .plan
                .document
                .steps
                .iter()
                .find(|s| s.key == execution.step)
        {
            execution.plan_revision_id = plan.id.clone();
            execution.node_id = step.node_id.clone();
            execution.contract_digest = configuration_hash(step);
        }
    }
    Ok(plan)
}

pub(crate) fn read_chunk(value: &Value, p: &Value) -> Result<Value> {
    let digest = configuration_hash(value);
    if let Some(expected) = p["value_digest"].as_str() {
        require(expected == digest, "读取快照已变化，请重新读取")?;
    }
    let body = value.to_string();
    let mut offset = (p["offset"].as_u64().unwrap_or(0) as usize).min(body.len());
    while !body.is_char_boundary(offset) {
        offset -= 1;
    }
    let budget = p["max_bytes"].as_u64().unwrap_or(16384).clamp(1024, 65536) as usize;
    let mut end = (offset + budget).min(body.len());
    while !body.is_char_boundary(end) {
        end -= 1;
    }
    Ok(
        json!({"value_digest":digest,"value_chunk":&body[offset..end],"offset":offset,"next_offset":if end<body.len(){Some(end)}else{None},"total_bytes":body.len()}),
    )
}

impl Engine {
    pub(crate) fn workflow_control_query(&self, method: &str, p: &Value) -> Option<Result<Value>> {
        if method == "workflow.rework.preview" {
            return Some(self.preview_native_rework(p));
        }
        if method == "workflow.change.preview" || method == "query.workflow_change_preview" {
            return Some(self.preview_native_workflow_change(p));
        }
        if ![
            "workflow.inspect",
            "query.workflow_control",
            "workflow.outputs.read",
            "workflow.events.read",
            "workflow.execution.inspect",
        ]
        .contains(&method)
        {
            return None;
        }
        Some(self.native_workflow_control_query(method, p))
    }
}
/// Read exact immutable outputs within a Workflow root without activating it.
pub fn read_workflow_outputs(
    s: &EngineeringState,
    instance: &ConfiguredWorkflowInstance,
    p: &Value,
) -> Result<Value> {
    let ids: Vec<String> = serde_json::from_value(p.get("ids").cloned().unwrap_or(json!([])))
        .map_err(|e| err("invalid_params", e.to_string()))?;
    require(ids.len() <= 100, "最多读取 100 项产物")?;
    let mut out = vec![];
    let count = ids.len().max(1);
    for id in ids {
        let r = s.configuration(&id)?;
        let same_root = if r.r#type == "typed_material" {
            r.data["root_instance_id"] == instance.root_instance_id
        } else {
            s.governance.iter().any(|owner| {
                owner.r#type == "workflow_instance"
                    && owner.data["root_instance_id"] == instance.root_instance_id
                    && owner.data["steps"].as_array().is_some_and(|steps| {
                        steps.iter().any(|e| e["id"] == r.data["execution_id"])
                    })
            }) || s.governance.iter().any(|e| {
                e.r#type == "workflow_execution_history"
                    && e.data["id"] == r.data["execution_id"]
                    && s.configuration(e.data["workflow_instance_id"].as_str().unwrap_or(""))
                        .is_ok_and(|owner| {
                            owner.data["root_instance_id"] == instance.root_instance_id
                        })
            })
        };
        require(
            r.scope.project_id() == Some(instance.project_id.as_str())
                && same_root
                && ["workflow_output", "typed_material"].contains(&r.r#type.as_str()),
            "产物不属于当前流程",
        )?;
        let value = r.data["value"]
            .pointer(p["pointer"].as_str().unwrap_or(""))
            .ok_or_else(|| err("invalid_params", "产物字段不存在"))?;
        let serialized = serde_json::to_string(value).unwrap();
        let budget = p["max_bytes"].as_u64().unwrap_or(16384).clamp(1024, 65536) as usize / count;
        let mut offset = (p["offset"].as_u64().unwrap_or(0) as usize).min(serialized.len());
        while !serialized.is_char_boundary(offset) {
            offset -= 1;
        }
        if serialized.len() > budget || offset > 0 {
            let mut end = (offset + budget).min(serialized.len());
            while !serialized.is_char_boundary(end) {
                end -= 1;
            }
            let mut data = r.data.clone();
            data.as_object_mut().unwrap().remove("value");
            out.push(json!({"id":r.id,"data":data,"value_chunk":&serialized[offset..end],"offset":offset,"next_offset":if end<serialized.len(){Some(end)}else{None},"total_bytes":serialized.len()}));
        } else {
            let mut scoped = json!(r);
            scoped["data"]["value"] = value.clone();
            out.push(scoped);
        }
    }
    Ok(json!({"outputs":out}))
}
