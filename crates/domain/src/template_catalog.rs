//! Directory membership is independent of immutable configuration and execution history.
use crate::*;

pub const TEMPLATE_CATALOG_STATE: &str = "template-catalog-state";
pub const TEMPLATE_INITIAL_GENERATION: u64 = 1;

pub fn template_catalog_state(s: &EngineeringState) -> Option<&ConfigurationRecord> {
    s.governance.iter().find(|r| r.id == TEMPLATE_CATALOG_STATE)
}
pub fn template_catalog_generation(s: &EngineeringState) -> u64 {
    template_catalog_state(s)
        .and_then(|r| r.data["generation"].as_u64())
        .unwrap_or(TEMPLATE_INITIAL_GENERATION)
}
pub fn template_definition_kind(kind: &str) -> bool {
    matches!(
        kind,
        "role"
            | "skill"
            | "spec"
            | "standard"
            | "workflow"
            | "workflow_draft"
            | "delivery_type"
            | "capability"
            | "agent"
            | "model"
            | "model_provider"
            | "model_catalog"
            | "harness"
            | "sandbox"
            | "tool"
            | "integration"
            | "integration_binding"
            | "permission_context"
            | "permission_profile"
    )
}
pub fn template_member_kind(kind: &str) -> &str {
    if kind == "workflow_draft" {
        "workflow"
    } else {
        kind
    }
}
pub fn template_catalog_member<'a>(
    s: &'a EngineeringState,
    r: &ConfigurationRecord,
) -> Option<&'a ConfigurationRecord> {
    s.governance
        .iter()
        .filter(|m| {
            m.kind == "template_catalog_member"
                && m.definition_id == r.id
                && m.data["kind"] == template_member_kind(&r.kind)
        })
        .max_by_key(|m| m.data["generation"].as_u64().unwrap_or(0))
}
pub fn template_record_current(s: &EngineeringState, r: &ConfigurationRecord) -> bool {
    if !template_definition_kind(&r.kind) {
        if matches!(
            r.kind.as_str(),
            "skill_package" | "integration_discovery" | "integration_disabled"
        ) {
            if let Some(owner) = s.governance.iter().find(|v| {
                v.id == r.definition_id
                    || (v.kind == "integration" && v.definition_id == r.definition_id)
            }) {
                return template_record_current(s, owner);
            }
            return template_catalog_generation(s) == TEMPLATE_INITIAL_GENERATION;
        }
        return true;
    }
    template_catalog_member(s, r)
        .and_then(|m| m.data["generation"].as_u64())
        .unwrap_or(TEMPLATE_INITIAL_GENERATION)
        == template_catalog_generation(s)
}
pub fn template_definition_write(method: &str) -> bool {
    let (family, action) = method.split_once('.').unwrap_or((method, ""));
    (template_definition_kind(family)
        && matches!(
            action,
            "create" | "revise" | "delete" | "retire" | "disable"
        ))
        || matches!(
            method,
            "configuration.retire"
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

/// Configuration projections used by workspace selectors belong to the same current directory.
pub fn template_projection_current(s: &EngineeringState, definition: &Definition) -> bool {
    if let Some(record) = s
        .governance
        .iter()
        .find(|r| r.id == definition.id && r.is_definition())
    {
        return template_record_current(s, record);
    }
    s.governance
        .iter()
        .find(|m| m.kind == "template_catalog_member" && m.definition_id == definition.id)
        .and_then(|m| m.data["generation"].as_u64())
        .unwrap_or(TEMPLATE_INITIAL_GENERATION)
        == template_catalog_generation(s)
}

/// Source comparison includes editable presentation metadata as well as execution content.
pub fn template_source_hash(record: &ConfigurationRecord) -> String {
    configuration_hash(&(&record.kind, &record.name, &record.scope, &record.data))
}

pub fn template_record_disabled(s: &EngineeringState, record: &ConfigurationRecord) -> bool {
    s.governance
        .iter()
        .any(|r| r.kind == "integration_disabled" && r.definition_id == record.definition_id)
}
