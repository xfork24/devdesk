//! Explicit current Workflow fixtures for structural and orchestration tests.
#![allow(dead_code)]
use devdesk_domain::*;
use serde_json::Value;
pub fn current(mut value: Value) -> Value {
    let version = value["schema_version"].as_u64();
    if !matches!(version, Some(2 | 3)) {
        return value;
    }
    value["schema_version"] = serde_json::json!(3);
    if let Some(steps) = value["steps"].as_array_mut() {
        for step in steps {
            let goal = step
                .get_mut("interaction")
                .and_then(Value::as_object_mut)
                .and_then(|v| v.remove("goal"));
            if version == Some(3) {
                continue;
            }
            let required = action_requires_task(step["kind"].as_str().unwrap_or(""));
            step["contract_version"] = serde_json::json!(if required { 2 } else { 1 });
            if required {
                step["task_instructions"] = serde_json::json!(goal.and_then(|v| v.as_str().map(str::to_owned)).filter(|v| !v.is_empty()).unwrap_or_else(||format!("Execute fixture step {} within its frozen scope and return the declared output contract.", step["key"])));
            }
            if let Some(nodes) = step
                .get_mut("execution_recipe")
                .and_then(Value::as_array_mut)
            {
                for node in nodes {
                    if recipe_node_is_agent(node["key"].as_str().unwrap_or("")) {
                        node["task_instructions"] = serde_json::json!(format!(
                            "Execute fixture node {} within the frozen task scope.",
                            node["key"]
                        ));
                    }
                }
            }
        }
    }
    value
}
