//! Template defaults are resolved from the active database catalog.
use crate::*;

pub(crate) fn role_deleted(s: &EngineeringState, role: &ConfigurationRecord) -> bool {
    s.governance
        .iter()
        .any(|r| r.kind == "role_retirement" && r.definition_id == role.definition_id)
}

impl Engine {
    pub fn configuration_defaults(&self, project: Option<&str>) -> Result<Value> {
        let s = self.store.snapshot()?;
        let mut defaults = current_template_defaults(&s)
            .ok_or_else(|| err("configuration_required", "模板目录尚未初始化"))?;
        let scope = project
            .map(|id| Scope::Project {
                project_id: id.into(),
            })
            .unwrap_or(Scope::Global);
        for agent in defaults["role_agent_bindings"]
            .as_object()
            .ok_or_else(|| err("configuration_required", "默认角色绑定缺失"))?
            .values()
        {
            let r =
                crate::governance::reference(&s, agent.as_str().unwrap_or(""), "agent", &scope)?;
            crate::governance::validate_composition(&s, &s.resolve_agent(&r.id)?, &scope)?;
        }
        crate::governance::reference(
            &s,
            defaults["workflow_ref"].as_str().unwrap_or(""),
            "workflow",
            &scope,
        )?;
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
                .filter(|r| r.kind == "harness_materialization"
                    && json!(r.definition_id) == defaults["harness_ref"])
                .collect::<Vec<_>>()
        );
        defaults["catalog_generation"] = json!(template_catalog_generation(&s));
        Ok(defaults)
    }
}
