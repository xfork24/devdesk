//! Typed source edges. Free text, literal inputs and example JSON never create dependencies.
use crate::*;
use serde_json::json;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigurationEdge {
    pub from: Id,
    pub path: String,
    pub relation: String,
    pub expected_type: String,
    pub to: Id,
}
pub fn configuration_edges(record: &ConfigurationRecord) -> Result<Vec<ConfigurationEdge>> {
    let mut edges = Vec::new();
    let mut add = |path: String, kind: &str, value: &Value| -> Result<()> {
        if value.is_null() {
            return Ok(());
        }
        let items: Vec<_> = match value {
            Value::Array(items) => items
                .iter()
                .enumerate()
                .map(|(i, v)| (format!("{path}/{i}"), v))
                .collect(),
            v => vec![(path, v)],
        };
        for (path, item) in items {
            let target = item.as_str().ok_or_else(|| {
                err(
                    "invalid_reference",
                    format!("{} {path}: 引用必须为字符串", record.id),
                )
            })?;
            parse_source_reference(target)
                .map_err(|e| err(&e.code, format!("{} {path}: {}", record.id, e.message)))?;
            edges.push(ConfigurationEdge {
                from: record.id.clone(),
                relation: if kind == "workflow" && record.r#type == "workflow" {
                    "subflow"
                } else {
                    "reference"
                }
                .into(),
                path,
                expected_type: kind.into(),
                to: target.into(),
            });
        }
        Ok(())
    };
    for (key, value) in &record.refs {
        let kind = match key.as_str() {
            "skills" => "skill",
            "tools" => "tool",
            "spec_refs" => "spec",
            "root_workflow_refs" => "workflow",
            "model_providers" => "model_provider",
            "model_catalog_ref" => "model_catalog",
            "runtime_resource_ref" => "runtime_resource",
            "harness_ref" => "harness",
            "model_ref" => "model",
            "role_ref" => "role",
            "capability_ref" => "capability",
            "sandbox_ref" => "sandbox",
            _ => return Err(err("invalid_reference", format!("未知引用字段 {key}"))),
        };
        add(format!("/{key}"), kind, value)?;
    }
    let data = &record.data;
    match record.r#type.as_str() {
        "harness" if !record.refs.contains_key("runtime_resource_ref") => {
            if let Some(digest) = data["recipe_digest"].as_str() {
                edges.push(ConfigurationEdge {
                    from: record.id.clone(),
                    path: "/data/recipe_digest".into(),
                    relation: "content_digest".into(),
                    expected_type: "runtime_resource".into(),
                    to: digest.into(),
                });
            }
        }
        "workflow" => {
            let _: WorkflowDefinition = record.decode()?;
            for (i, stage) in data["stages"].as_array().into_iter().flatten().enumerate() {
                add(
                    format!("/data/stages/{i}/default_role_ref"),
                    "role",
                    &stage["default_role_ref"],
                )?;
            }
            for (i, step) in data["steps"].as_array().into_iter().flatten().enumerate() {
                for (key, kind) in [
                    ("role_ref", "role"),
                    ("tool_ref", "tool"),
                    ("workflow_ref", "workflow"),
                ] {
                    add(
                        format!("/data/steps/{i}/executor/{key}"),
                        kind,
                        &step["executor"][key],
                    )?;
                }
                for (key, kind) in [("required_skill_refs", "skill"), ("spec_refs", "spec")] {
                    add(format!("/data/steps/{i}/{key}"), kind, &step[key])?;
                }
                for (name, port) in step["outputs"].as_object().into_iter().flatten() {
                    add(
                        format!("/data/steps/{i}/outputs/{name}/delivery_ref"),
                        "delivery_type",
                        &port["delivery_ref"],
                    )?;
                }
                add(
                    format!("/data/steps/{i}/access/tool_refs"),
                    "tool",
                    &step["access"]["tool_refs"],
                )?;
            }
            for (name, port) in data["inputs"].as_object().into_iter().flatten() {
                add(
                    format!("/data/inputs/{name}/delivery_ref"),
                    "delivery_type",
                    &port["delivery_ref"],
                )?;
            }
        }
        "permission_context" => {
            for layer in ["harness", "sandbox"] {
                add(
                    format!("/data/{layer}/binding_revision_id"),
                    "permission_binding",
                    &data[layer]["binding_revision_id"],
                )?;
            }
            for (i, grant) in data["step_grants"]
                .as_array()
                .into_iter()
                .flatten()
                .enumerate()
            {
                add(
                    format!("/data/step_grants/{i}/workflow_ref"),
                    "workflow",
                    &grant["workflow_ref"],
                )?;
                add(
                    format!("/data/step_grants/{i}/tool_refs"),
                    "tool",
                    &grant["tool_refs"],
                )?;
            }
        }
        "model_catalog" => {
            for (i, p) in data["providers"]
                .as_array()
                .into_iter()
                .flatten()
                .enumerate()
            {
                add(
                    format!("/data/providers/{i}/provider_ref"),
                    "model_provider",
                    &p["provider_ref"],
                )?;
            }
        }
        "tool" | "integration_binding" => {
            add(
                "/data/integration_ref".into(),
                "integration",
                &data["integration_ref"],
            )?;
        }
        "agent_instance" => {
            add(
                "/data/agent_revision_id".into(),
                "agent",
                &data["agent_revision_id"],
            )?;
        }
        "permission_binding" => {
            add(
                "/data/permission_revision_id".into(),
                "permission_profile",
                &data["permission_revision_id"],
            )?;
            let kind = if data["target"]["kind"] == "harness" {
                "harness"
            } else {
                "sandbox"
            };
            add(
                "/data/target/revision_id".into(),
                kind,
                &data["target"]["revision_id"],
            )?;
        }
        "project_configuration" | "work_configuration" => {
            add(
                "/data/orchestration/policy_ref".into(),
                "orchestration_policy",
                &data["orchestration"]["policy_ref"],
            )?;
            for (key, kind) in [
                ("workflow_ref", "workflow"),
                ("standard_ref", "standard"),
                ("permission_context_ref", "permission_context"),
                ("orchestrator_role_ref", "role"),
            ] {
                add(format!("/data/{key}"), kind, &data[key])?;
            }
            for (role, agent) in data["role_agent_bindings"]
                .as_object()
                .into_iter()
                .flatten()
            {
                add(
                    format!("/data/role_agent_bindings/{role}/role"),
                    "role",
                    &json!(role),
                )?;
                add(format!("/data/role_agent_bindings/{role}"), "agent", agent)?;
            }
        }
        "integration_project_configuration" => {
            for (key, binding) in data.as_object().into_iter().flatten() {
                add(format!("/data/{key}"), "integration_binding", binding)?;
            }
        }
        "skill_package" => {
            for (i, tool) in data["requires_tools"]
                .as_array()
                .into_iter()
                .flatten()
                .enumerate()
            {
                if tool
                    .as_str()
                    .is_some_and(|v| parse_source_reference(v).is_ok())
                {
                    add(format!("/data/requires_tools/{i}"), "tool", tool)?;
                }
            }
        }
        _ => {}
    }
    Ok(edges)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn text_and_examples_are_not_dependencies_and_nested_ports_are() {
        let source=SourceDefinition{r#type:"workflow".into(),identity:"flow".into(),revision:1,name:"Flow".into(),scope:Scope::global(),refs:References::new(),data:json!({"schema_version":999,"stages":[],"steps":[{"key":"one","name":"Literal spec:1","stage":"","executor":{"type":"control"},"task_instructions":"skill:7","inputs":{"example":{"type":"constant","value":{"tool_ref":"fake:1"}}},"outputs":{"report":{"delivery_ref":"delivery:2"}}}]})}.into_record().unwrap();
        let edges = configuration_edges(&source).unwrap();
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].to, "delivery:2");
        assert_eq!(edges[0].expected_type, "delivery_type");
    }
}
