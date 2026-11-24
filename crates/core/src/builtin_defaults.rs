//! Template defaults are resolved from the active database catalog.
use crate::*;

pub(crate) fn role_deleted(s: &EngineeringState, role: &ConfigurationRecord) -> bool {
    s.governance
        .iter()
        .any(|r| r.r#type == "role_retirement" && r.definition_id == role.definition_id)
}

impl Engine {
    pub fn configuration_defaults(&self, project: Option<&str>) -> Result<Value> {
        self.configuration_defaults_view(project, true)
    }
    pub fn configuration_default_selection(&self, project: Option<&str>) -> Result<Value> {
        self.configuration_defaults_view(project, false)
    }
    fn configuration_defaults_view(&self, project: Option<&str>, full: bool) -> Result<Value> {
        let s = self.store.configuration_snapshot()?.state.as_ref().clone();
        let mut defaults = current_template_defaults(&s)
            .ok_or_else(|| err("configuration_required", "模板目录尚未初始化"))?;
        let scope = project.map(Scope::project).unwrap_or(Scope::global());
        for agent in defaults["role_agent_bindings"]
            .as_object()
            .into_iter()
            .flat_map(|rows| rows.values())
        {
            let r =
                crate::governance::reference(&s, agent.as_str().unwrap_or(""), "agent", &scope)?;
            crate::governance::validate_composition(&s, &s.resolve_agent(&r.id)?, &scope)?;
        }
        if let Some(workflow) = defaults["workflow_ref"].as_str() {
            crate::governance::reference(&s, workflow, "workflow", &scope)?;
        }
        if full {
            defaults["records"] = json!(
                crate::templates::pinned_template_records(&s, Some(&defaults))
                    .into_iter()
                    .filter(|r| configuration_visible(&s, r, project))
                    .collect::<Vec<_>>()
            );
            defaults["sandboxes"] = json!(
                s.sandboxes
                    .iter()
                    .filter(|b| json!(b.id) == defaults["sandbox_ref"])
                    .collect::<Vec<_>>()
            );
            defaults["materializations"] = json!(
                s.governance
                    .iter()
                    .filter(|r| r.r#type == "harness_materialization"
                        && json!(r.definition_id) == defaults["harness_ref"])
                    .collect::<Vec<_>>()
            );
        }
        defaults["catalog_generation"] = json!(template_catalog_generation(&s));
        Ok(defaults)
    }
}
