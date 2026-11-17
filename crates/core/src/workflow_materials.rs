//! Read-only material index over current and historical executions and frozen output contracts.
use crate::*;
use std::collections::BTreeSet;

struct Material<'a> {
    metadata: Value,
    output: &'a ConfigurationRecord,
    definitions: &'a Value,
}

impl Engine {
    pub(crate) fn workflow_materials(&self, p: &Value) -> Result<Value> {
        let state = self.store.snapshot()?;
        let scope = crate::workflow_reading::workflow_read_scope(&state, p)?;
        let project = scope.project.as_str();
        let owners = &scope.owners;
        if let Some(id) = p["output_id"].as_str()
            && let Some(document) = state.documents.iter().find(|document| {
                document.id == id
                    && owners.contains_key(
                        document
                            .task_id
                            .as_deref()
                            .or(document.feature_id.as_deref())
                            .or(document.project_id.as_deref())
                            .unwrap_or(""),
                    )
            })
        {
            return Ok(json!({"document":document}));
        }
        let mut materials = Vec::new();
        let mut seen = BTreeSet::new();
        for entry in crate::workflow_reading::read_executions(&state, &scope) {
            let instance = entry.instance;
            let execution = entry.execution;
            if instance.scope.project_id() != Some(project) {
                return Err(err("scope_violation", "工作流材料跨项目"));
            }
            let owner_id = instance.data["owner_id"].as_str().unwrap();
            let (owner_kind, owner_title) = &owners[owner_id];
            for (port, output_id) in execution["outputs"].as_object().into_iter().flatten() {
                let output_id = output_id
                    .as_str()
                    .ok_or_else(|| err("storage", "产物引用无效"))?;
                let output = state.configuration(output_id)?;
                if output.r#type != "workflow_output" || output.scope.project_id() != Some(project)
                {
                    return Err(err("scope_violation", "产物不属于当前执行"));
                }
                // Reused outputs still link to the execution that produced
                // them; historical rounds remain readable after rework.
                if output.data["execution_id"] != execution["id"] || !seen.insert(output_id) {
                    continue;
                }
                let (plan, step) = crate::workflow_reading::frozen_step(&state, &entry)?;
                let delivery_ref = output.data["delivery_ref"]
                    .as_str()
                    .ok_or_else(|| err("storage", "交付类型引用缺失"))?;
                let definitions = &plan["definitions"];
                let delivery = &definitions[delivery_ref];
                if delivery["type"] != "delivery_type" {
                    return Err(err("not_found", "冻结交付类型不存在"));
                }
                let stage = plan["document"]["stages"]
                    .as_array()
                    .and_then(|stages| stages.iter().find(|stage| stage["key"] == step["stage"]));
                let turn_id = output.data["provenance"]["turn_id"]
                    .as_str()
                    .or(execution["turn_id"].as_str());
                let session_id = turn_id
                    .and_then(|id| {
                        state
                            .turns
                            .iter()
                            .find(|turn| turn.id == id)
                            .map(|turn| turn.session_id.as_str())
                    })
                    .or(execution["session_id"].as_str());
                materials.push(Material {
                        metadata: json!({
                            "id":output.id,"owner_id":owner_id,"owner_kind":owner_kind,"owner_title":owner_title,
                            "instance_id":instance.id,"workflow_ref":instance.data["workflow_ref"],"workflow_name":instance.name,
                            "execution_id":execution["id"],"step":execution["step"],"step_name":step["name"],"path":execution["path"],
                            "stage_name":stage.map(|stage| &stage["name"]),"status":execution["status"],"round":execution["round"],
                            "port":port,"delivery_ref":delivery_ref,"delivery_name":delivery["name"],"format":delivery["data"]["format"],
                            "created_at":output.created_at,"plan_revision_id":execution["plan_revision_id"],
                            "session_id":session_id,"turn_id":turn_id,"historical":entry.historical,
                            "feature_id":state.tasks.iter().find(|task|task.id==owner_id).map(|task|task.feature_id.as_str()).or_else(||state.features.iter().find(|feature|feature.id==owner_id).map(|feature|feature.id.as_str()))
                        }),
                        output,
                        definitions,
                    });
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
        let documents: Vec<_> = state
            .documents
            .iter()
            .filter(|document| {
                owners.contains_key(
                    document
                        .task_id
                        .as_deref()
                        .or(document.feature_id.as_deref())
                        .or(document.project_id.as_deref())
                        .unwrap_or(""),
                ) && (query.is_empty() || document.title.to_lowercase().contains(&query))
            })
            .collect();
        let offset = p["offset"].as_u64().unwrap_or(0) as usize;
        let limit = p["limit"].as_u64().unwrap_or(20).clamp(1, 100) as usize;
        Ok(
            json!({"records":materials.into_iter().skip(offset).take(limit).map(|row| row.metadata).collect::<Vec<_>>(),"documents":documents,"total":total,"offset":offset,"limit":limit}),
        )
    }
}
