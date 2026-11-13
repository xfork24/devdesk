use crate::*;
pub fn template_definition_write(method: &str) -> bool {
    let (family, action) = method.split_once('.').unwrap_or((method, ""));
    (template_definition_kind(family)
        && matches!(
            action,
            "create" | "revise" | "delete" | "retire" | "disable"
        ))
        || matches!(
            method,
            "configuration.transfer.stage"
                | "configuration.transfer.apply"
                | "configuration.transfer.cancel"
                | "configuration.change.apply"
                | "configuration.retire"
                | "workflow.publish"
                | "workflow.draft.create"
                | "workflow.draft.update"
                | "workflow.draft.upgrade"
                | "skill.import"
                | "skill.import_package"
                | "integration.openapi.import"
                | "integration.tools.publish"
                | "integration.binding.create"
                | "integration.binding.revise"
                | "integration.tool.create"
                | "integration.tool.revise"
                | "permission.profile.create"
                | "permission.profile.revise"
                | "permission.profile.import"
                | "permission.profile.reload"
        )
}

pub fn template_catalog_query(method: &str) -> bool {
    matches!(
        method,
        "query.workspace"
            | "query.project_configuration"
            | "query.configurations"
            | "query.roles"
            | "query.capabilities"
            | "query.workflows"
            | "query.integrations"
            | "query.integration_bindings"
            | "query.permission_resources"
            | "query.configuration_defaults"
            | "query.template_catalog"
            | "query.template_reset_preview"
    )
}
