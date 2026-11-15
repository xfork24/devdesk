//! Read-only material index over current executions, preserving frozen output contracts.
use crate::*;
use std::collections::{BTreeMap, BTreeSet};

struct Material<'a> {
    metadata: Value,
    output: &'a ConfigurationRecord,
    definitions: &'a Value,
}

impl Engine {
    pub(crate) fn workflow_materials(&self, p: &Value) -> Result<Value> {
        let state = self.store.snapshot()?;
        let owner = text(p, "owner_id")?;
        let work = work_reference(&state, &owner)?;
        let scope = work.scope(&state)?;
        let project = scope.project_id().unwrap();
        if p["project_id"].as_str().is_some_and(|id| id != project) {
            return Err(err("scope_violation", "材料不属于当前项目"));
        }
        let mut owners = BTreeMap::new();
        match work {
            WorkReference::Feature { ref id } => {
                owners.insert(id.as_str(), ("feature", state.feature(id)?.title.as_str()));
                for task in state.tasks.iter().filter(|task| task.feature_id == *id) {
                    owners.insert(task.id.as_str(), ("task", task.title.as_str()));
                }
            }
            WorkReference::Task { ref id } => {
                owners.insert(id.as_str(), ("task", state.task(id)?.title.as_str()));
            }
            WorkReference::Project { ref id } => {
                let project = state.projects.iter().find(|row| row.id == *id).unwrap();
                owners.insert(id.as_str(), ("project", project.title.as_str()));
            }
        }
        if let Some(id) = p["output_id"].as_str() {
            if let Some(document) = state.documents.iter().find(|document| {
                document.id == id
                    && owners.contains_key(
                        document
                            .task_id
                            .as_deref()
                            .or(document.feature_id.as_deref())
                            .or(document.project_id.as_deref())
                            .unwrap_or(""),
                    )
            }) {
                return Ok(json!({"document":document}));
            }
        }
        let mut selected: BTreeSet<&str> = owners
            .keys()
            .filter_map(|id| crate::standards::current_work_instance(&state, id))
            .map(|row| row.id.as_str())
            .collect();
        // Nested Workflows need not be separate work roots. Follow only current
        // parent executions, excluding children superseded by rework or a new plan.
        loop {
            let executions: BTreeSet<&str> = state
                .governance
                .iter()
                .filter(|row| selected.contains(row.id.as_str()))
                .flat_map(|row| row.data["steps"].as_array().into_iter().flatten())
                .filter_map(|step| step["id"].as_str())
                .collect();
            let before = selected.len();
            selected.extend(
                state
                    .governance
                    .iter()
                    .filter(|row| {
                        row.r#type == "workflow_instance"
                            && row.scope.project_id() == Some(project)
                            && row.data["owner_id"]
                                .as_str()
                                .is_some_and(|id| owners.contains_key(id))
                            && row.data["parent_execution_id"]
                                .as_str()
                                .is_some_and(|id| executions.contains(id))
                    })
                    .map(|row| row.id.as_str()),
            );
            if selected.len() == before {
                break;
            }
        }
        let mut materials = Vec::new();
        let mut seen = BTreeSet::new();
        for instance in state
            .governance
            .iter()
            .filter(|row| selected.contains(row.id.as_str()))
        {
            if instance.scope.project_id() != Some(project) {
                return Err(err("scope_violation", "工作流材料跨项目"));
            }
            let owner_id = instance.data["owner_id"].as_str().unwrap();
            let (owner_kind, owner_title) = owners[owner_id];
            for execution in instance.data["steps"].as_array().into_iter().flatten() {
                for (port, output_id) in execution["outputs"].as_object().into_iter().flatten() {
                    let output_id = output_id
                        .as_str()
                        .ok_or_else(|| err("storage", "产物引用无效"))?;
                    if !seen.insert(output_id) {
                        continue;
                    }
                    let output = state.configuration(output_id)?;
                    if output.r#type != "workflow_output"
                        || output.scope.project_id() != Some(project)
                        || output.data["execution_id"] != execution["id"]
                    {
                        return Err(err("scope_violation", "产物不属于当前执行"));
                    }
                    let plan = state.configuration(
                        execution["plan_revision_id"]
                            .as_str()
                            .ok_or_else(|| err("storage", "原始计划引用缺失"))?,
                    )?;
                    if plan.r#type != "workflow_plan_revision"
                        || plan.data["instance_id"] != instance.id
                    {
                        return Err(err("scope_violation", "原始计划不属于当前流程"));
                    }
                    let plan = &plan.data["plan"];
                    let step = plan["document"]["steps"]
                        .as_array()
                        .and_then(|steps| {
                            steps
                                .iter()
                                .find(|step| step["node_id"] == execution["node_id"])
                        })
                        .ok_or_else(|| err("not_found", "原始步骤合同不存在"))?;
                    let delivery_ref = output.data["delivery_ref"]
                        .as_str()
                        .ok_or_else(|| err("storage", "交付类型引用缺失"))?;
                    let definitions = &plan["definitions"];
                    let delivery = &definitions[delivery_ref];
                    if delivery["type"] != "delivery_type" {
                        return Err(err("not_found", "冻结交付类型不存在"));
                    }
                    let stage = plan["document"]["stages"].as_array().and_then(|stages| {
                        stages.iter().find(|stage| stage["key"] == step["stage"])
                    });
                    materials.push(Material {
                        metadata: json!({
                            "id":output.id,"owner_id":owner_id,"owner_kind":owner_kind,"owner_title":owner_title,
                            "instance_id":instance.id,"workflow_ref":instance.data["workflow_ref"],"workflow_name":instance.name,
                            "execution_id":execution["id"],"step":execution["step"],"step_name":step["name"],"path":execution["path"],
                            "stage_name":stage.map(|stage| &stage["name"]),"status":execution["status"],"round":execution["round"],
                            "port":port,"delivery_ref":delivery_ref,"delivery_name":delivery["name"],"format":delivery["data"]["format"],
                            "created_at":output.created_at,"plan_revision_id":execution["plan_revision_id"]
                        }),
                        output,
                        definitions,
                    });
                }
            }
        }
        materials.sort_by(|a, b| {
            a.output
                .created_at
                .cmp(&b.output.created_at)
                .then_with(|| a.output.id.cmp(&b.output.id))
        });
        if let Some(id) = p["output_id"].as_str() {
            let material = materials
                .iter()
                .find(|row| row.output.id == id)
                .ok_or_else(|| err("not_found", "产物不属于当前工作流程，请刷新材料"))?;
            let delivery_ref = material.output.data["delivery_ref"].as_str().unwrap();
            let delivery = &material.definitions[delivery_ref];
            let mut definitions = serde_json::Map::new();
            definitions.insert(delivery_ref.into(), delivery.clone());
            for reference in delivery["spec_refs"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
            {
                let spec = &material.definitions[reference];
                if spec["type"] != "spec" {
                    return Err(err("not_found", "冻结内容规范不存在"));
                }
                definitions.insert(reference.into(), spec.clone());
            }
            return Ok(
                json!({"record":material.metadata,"output":material.output.data,"definitions":definitions}),
            );
        }
        let query: String = p["search"]
            .as_str()
            .unwrap_or("")
            .chars()
            .take(200)
            .collect::<String>()
            .to_lowercase();
        materials.retain(|row| {
            query.is_empty()
                || [
                    "delivery_name",
                    "step_name",
                    "stage_name",
                    "owner_title",
                    "port",
                ]
                .iter()
                .any(|key| {
                    row.metadata[key]
                        .as_str()
                        .is_some_and(|text| text.to_lowercase().contains(&query))
                })
        });
        if let Some(execution) = p["execution_id"].as_str() {
            materials.retain(|row| row.metadata["execution_id"] == execution);
        }
        let total = materials.len();
        let offset = p["offset"].as_u64().unwrap_or(0) as usize;
        let limit = p["limit"].as_u64().unwrap_or(20).clamp(1, 100) as usize;
        Ok(
            json!({"records":materials.into_iter().skip(offset).take(limit).map(|row| row.metadata).collect::<Vec<_>>(),"total":total,"offset":offset,"limit":limit}),
        )
    }
}
