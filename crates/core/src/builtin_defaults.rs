//! Current built-in configuration lookup.
use crate::*;
pub(crate) fn role_deleted(s: &EngineeringState, role: &ConfigurationRecord) -> bool {
    s.governance
        .iter()
        .any(|r| r.kind == "role_retirement" && r.definition_id == role.definition_id)
}

impl Engine {
    pub fn configuration_defaults(&self, project: Option<&str>) -> Result<Value> {
        let s = self.store.snapshot()?;
        let catalog = crate::current_catalog::catalog();
        let current = |id: &str| -> Result<String> {
            let source = catalog.configuration(id)?;
            let latest = s
                .governance
                .iter()
                .filter(|r| {
                    r.kind == source.kind && r.definition_id == source.definition_id && r.immutable
                })
                .max_by_key(|r| r.version)
                .ok_or_else(|| err("not_found", format!("默认配置不存在: {id}")))?;
            require(
                !crate::configuration::retired(&s, latest),
                "默认配置已停用，请选择当前可用配置",
            )?;
            Ok(latest.id.clone())
        };
        let model = current(DEFAULT_MODEL_ID)?;
        let harness = current(DEFAULT_HARNESS_ID)?;
        let workflow = current(DEFAULT_FEATURE_WORKFLOW_V3_ID)?;
        let task_workflow = current(DEFAULT_TASK_WORKFLOW_V3_ID)?;
        let capability = current(DEFAULT_CAPABILITY_ID)?;
        let scope = project
            .map(|id| Scope::Project {
                project_id: id.into(),
            })
            .unwrap_or(Scope::Global);
        let mut roles = serde_json::Map::new();
        let mut ids = vec![
            workflow.clone(),
            task_workflow.clone(),
            model.clone(),
            harness.clone(),
            capability.clone(),
        ];
        for role in [
            "analyst",
            "architect",
            "project-manager",
            "developer",
            "tester",
            "reviewer",
        ] {
            let r = s.configuration(&current(&default_role_id(role))?)?;
            let agent = current(&default_configured_agent_id(role))?;
            let configured = crate::governance::reference(&s, &agent, "agent", &scope)?;
            crate::governance::validate_composition(&s, &configured.decode()?, &scope)?;
            roles.insert(r.definition_id.clone(), json!(agent));
            ids.push(agent);
        }
        crate::governance::reference(&s, &workflow, "workflow", &scope)?;
        fn collect(value: &Value, ids: &mut Vec<String>) {
            match value {
                Value::String(id) => {
                    if !ids.contains(id) {
                        ids.push(id.clone());
                    }
                }
                Value::Array(values) => {
                    for value in values {
                        collect(value, ids);
                    }
                }
                Value::Object(values) => {
                    for value in values.values() {
                        collect(value, ids);
                    }
                }
                _ => {}
            }
        }
        loop {
            let before = ids.len();
            for record in &s.governance {
                if ids.contains(&record.id) || ids.contains(&record.definition_id) {
                    collect(&record.data, &mut ids);
                }
            }
            if before == ids.len() {
                break;
            }
        }

        Ok(
            json!({"workflow_ref":workflow,"task_workflow_ref":task_workflow,"role_agent_bindings":roles,"standard_ref":null,"capability_ref":capability,"model_ref":model,"harness_ref":harness,"sandbox_ref":DEFAULT_SANDBOX_ID,"records":s.governance.iter().filter(|r|ids.contains(&r.id)||ids.contains(&r.definition_id)).collect::<Vec<_>>(),"sandboxes":s.sandboxes.iter().filter(|b|b.id==DEFAULT_SANDBOX_ID).collect::<Vec<_>>(),"materializations":s.governance.iter().filter(|r|r.kind=="harness_materialization"&&(r.definition_id==harness||r.definition_id==DEFAULT_HARNESS_ID)).collect::<Vec<_>>() }),
        )
    }
}
