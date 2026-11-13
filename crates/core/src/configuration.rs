//! Explicit reusable capabilities, visibility and configuration reference checks.
use crate::governance::reference;
use crate::*;

pub(crate) fn role_record<'a>(
    s: &'a EngineeringState,
    key: &str,
) -> Result<&'a ConfigurationRecord> {
    s.governance
        .iter()
        .filter(|r| r.r#type == "role" && r.id == key)
        .max_by_key(|r| r.version)
        .ok_or_else(|| err("not_found", format!("Role 不存在: {key}")))
}
pub fn visible(s: &EngineeringState, r: &ConfigurationRecord, project: Option<&str>) -> bool {
    if project.is_none() {
        return true;
    }
    if r.r#type == "role" {
        let current = r;
        return current.decode::<RoleDefinition>().ok().is_some_and(|role| {
            role.visibility.as_ref().map_or_else(
                || {
                    current
                        .scope
                        .visible_from(&Scope::project(project.unwrap()))
                },
                |v| v.permits(project),
            )
        });
    }
    if !r.scope.visible_from(&Scope::project(project.unwrap())) {
        return false;
    }
    if r.r#type == "agent" {
        if let Some(key) = reference_value(&r.refs, "role_ref").as_str() {
            if !role_record(s, key).is_ok_and(|r| visible(s, r, project)) {
                return false;
            }
        }
        if let Some(key) = reference_value(&r.refs, "capability_ref").as_str() {
            if !s.configuration(key).is_ok_and(|r| visible(s, r, project)) {
                return false;
            }
        }
    }
    true
}
pub(crate) fn normalize_agent(
    s: &EngineeringState,
    refs: &References,
    scope: &Scope,
) -> Result<()> {
    reference(s, &reference_id(refs, "role_ref")?, "role", scope)?;
    reference(
        s,
        &reference_id(refs, "capability_ref")?,
        "capability",
        scope,
    )?;
    Ok(())
}
pub(crate) fn validate_capability(
    s: &EngineeringState,
    refs: &References,
    scope: &Scope,
) -> Result<()> {
    let skills = reference_list(refs, "skills")?;
    let tools = reference_list(refs, "tools")?;
    let model_ref = reference_id(refs, "model_ref")?;
    let harness_ref = reference_id(refs, "harness_ref")?;
    for skill in &skills {
        if let Some(r) = s
            .governance
            .iter()
            .find(|r| r.r#type == "skill_package" && r.definition_id == *skill)
        {
            let package: SkillPackage = r.decode()?;
            for dependency in &package.requires_tools {
                require(
                    tools.iter().any(|id| {
                        id == dependency
                            || s.configuration(id)
                                .is_ok_and(|r| r.definition_id == *dependency)
                    }),
                    format!("Skill 缺少工具依赖: {dependency}"),
                )?;
            }
        }
    }
    let model: ModelDefinition = reference(s, &model_ref, "model", scope)?.decode()?;
    let harness_record = reference(s, &harness_ref, "harness", scope)?;
    let harness: HarnessDefinition = harness_record.decode()?;
    require(
        !model.model.trim().is_empty() && !model.provider.trim().is_empty(),
        "Model 后端与标识不能为空",
    )?;

    s.resolve_model_connection(&model, harness_record)?;

    for tool in &tools {
        let record = reference(s, tool, "tool", scope)?;
        if record.data["integration_ref"].is_string() {
            let t: ExternalTool = record.decode()?;
            t.validate()?;
            reference(s, &t.integration_ref, "integration", scope)?;
            require(harness.provider == "codex", "此 Harness 尚不支持外部工具")?;
            continue;
        }
        let t: ToolDefinition = record.decode()?;
        if t.implementation == "platform" {
            require(harness.provider == "codex", "此 Harness 尚不支持平台工具桥")?;
            require(
                crate::platform_tools::PLATFORM_HANDLERS.contains(&t.qualified_name.as_str()),
                "平台工具处理器不可用",
            )?;
            continue;
        }
        require(
            t.implementation == "harness.native",
            "Tool 尚无可执行处理器",
        )?;
        require(
            t.harness_provider
                .as_ref()
                .is_none_or(|p| p == &harness.provider),
            "原生工具组不适用于所选 Harness",
        )?;
        require(
            t.capabilities
                .iter()
                .all(|c| harness.capabilities.contains(c)),
            "Harness 不支持 Tool 所需能力",
        )?;
    }
    require(
        tools
            .iter()
            .filter_map(|id| s.configuration(id).ok())
            .filter_map(|r| r.decode::<ToolDefinition>().ok())
            .any(|t| t.implementation == "harness.native"),
        "当前 Harness 必须显式声明原生 Tool 集合",
    )?;
    for skill in &skills {
        require(
            s.definitions
                .iter()
                .any(|d| &d.id == skill && d.family == "skill")
                || s.governance
                    .iter()
                    .any(|r| &r.id == skill && r.r#type == "skill" && r.scope.visible_from(scope)),
            "Skill 修订不存在",
        )?;
    }
    Ok(())
}
fn references_value(value: &Value, ids: &[&str]) -> bool {
    match value {
        Value::String(s) => ids.contains(&s.as_str()),
        Value::Array(v) => v.iter().any(|v| references_value(v, ids)),
        Value::Object(v) => v.values().any(|v| references_value(v, ids)),
        _ => false,
    }
}
pub(crate) fn references(s: &EngineeringState, source: &ConfigurationRecord) -> Vec<Value> {
    reference_users(s, source, false)
}
pub(crate) fn integration_reference_users(
    s: &EngineeringState,
    source: &ConfigurationRecord,
) -> Vec<Value> {
    reference_users(s, source, true)
}
fn reference_users(
    s: &EngineeringState,
    source: &ConfigurationRecord,
    recursive: bool,
) -> Vec<Value> {
    fn contains(
        s: &EngineeringState,
        value: &Value,
        ids: &[&str],
        seen: &mut std::collections::BTreeSet<Id>,
    ) -> bool {
        match value {
            Value::String(id) => {
                ids.contains(&id.as_str())
                    || (seen.insert(id.clone())
                        && s.configuration(id).is_ok_and(|r| {
                            contains(s, &json!({"data":r.data,"refs":r.refs}), ids, seen)
                        }))
            }
            Value::Array(values) => values.iter().any(|v| contains(s, v, ids, seen)),
            Value::Object(values) => values.values().any(|v| contains(s, v, ids, seen)),
            _ => false,
        }
    }
    let mut ids = vec![source.id.as_str(), source.definition_id.as_str()];
    if source.r#type == "role" {
        ids.extend(
            s.governance
                .iter()
                .filter(|r| r.r#type == "role" && r.definition_id == source.definition_id)
                .map(|r| r.id.as_str()),
        );
    }
    s.governance
        .iter()
        .filter(|r| {
            r.id != source.id
                && (r.is_definition()
                    || [
                        "project_configuration",
                        "integration_project_configuration",
                        "skill_package",
                    ]
                    .contains(&r.r#type.as_str()))
        })
        .filter(|r| {
            !s.governance.iter().any(|next| {
                next.r#type == r.r#type
                    && next.definition_id == r.definition_id
                    && next.version > r.version
            })
        })
        .filter(|r| {
            !s.governance.iter().any(|t| {
                (t.r#type == "configuration_retirement" && t.definition_id == r.id)
                    || (t.r#type == "role_retirement" && t.definition_id == r.definition_id)
            })
        })
        .filter(|r| {
            r.r#type != "skill_package"
                || s.configuration(&r.definition_id).is_ok_and(|owner| {
                    !retired(s, owner)
                        && !s.governance.iter().any(|next| {
                            next.r#type == "skill"
                                && next.definition_id == owner.definition_id
                                && next.version > owner.version
                        })
                })
        })
        .filter(|r| {
            let payload = json!({"data":r.data,"refs":r.refs});
            if recursive {
                contains(s, &payload, &ids, &mut Default::default())
            } else {
                references_value(&payload, &ids)
            }
        })
        .map(|r| json!({"id":r.id,"name":r.name,"type":r.r#type,"scope":r.scope}))
        .collect()
}
pub(crate) fn ensure_unreferenced(
    s: &EngineeringState,
    source: &ConfigurationRecord,
) -> Result<()> {
    let refs = references(s, source);
    require(
        refs.is_empty(),
        format!(
            "请先替换在用引用: {}",
            refs.iter()
                .filter_map(|r| r["name"].as_str())
                .collect::<Vec<_>>()
                .join("、")
        ),
    )
}
impl Engine {
    pub(crate) fn configuration_query(&self, method: &str, p: &Value) -> Option<Result<Value>> {
        if !["query.configuration_references", "query.capabilities"].contains(&method) {
            return None;
        }
        Some((|| {
            let s = self.store.snapshot()?;
            if method == "query.configuration_references" {
                return Ok(json!({"references":references(&s,s.configuration(&text(p,"id")?)?)}));
            }
            let rows: Vec<_> = s
                .governance
                .iter()
                .filter(|r| {
                    r.r#type == "capability"
                        && (p["include_archived"] == true || template_record_current(&s, r))
                        && !retired(&s, r)
                        && visible(&s, r, p["project_id"].as_str())
                })
                .filter(|r| {
                    !s.governance.iter().any(|n| {
                        n.r#type == r.r#type
                            && n.definition_id == r.definition_id
                            && n.version > r.version
                            && (p["include_archived"] == true || template_record_current(&s, n))
                    })
                })
                .collect();
            Ok(json!({"records":rows,"total":rows.len()}))
        })())
    }
}

pub fn project_configuration_references(
    s: &EngineeringState,
    source: &ConfigurationRecord,
) -> Vec<Value> {
    fn contains(
        s: &EngineeringState,
        value: &Value,
        source: &ConfigurationRecord,
        seen: &mut std::collections::BTreeSet<Id>,
    ) -> bool {
        match value {
            Value::String(id) => {
                if id == &source.id || id == &source.definition_id {
                    return true;
                }
                if !seen.insert(id.clone()) {
                    return false;
                }
                s.configuration(id).is_ok_and(|r| {
                    r.definition_id == source.definition_id
                        || contains(s, &json!({"data":r.data,"refs":r.refs}), source, seen)
                })
            }
            Value::Array(items) => items.iter().any(|v| contains(s, v, source, seen)),
            Value::Object(fields) => fields
                .iter()
                .any(|(k, v)| k == &source.definition_id || contains(s, v, source, seen)),
            _ => false,
        }
    }
    s.governance
        .iter()
        .filter(|r| {
            ["project_configuration", "integration_project_configuration"]
                .contains(&r.r#type.as_str())
                && !s.governance.iter().any(|next| {
                    next.r#type == r.r#type
                        && next.definition_id == r.definition_id
                        && next.version > r.version
                })
        })
        .filter(|r| {
            contains(
                s,
                &json!({"data":r.data,"refs":r.refs}),
                source,
                &mut Default::default(),
            )
        })
        .map(|r| json!({"id":r.id,"name":r.name,"type":r.r#type,"scope":r.scope}))
        .collect()
}

pub fn retired(s: &EngineeringState, r: &ConfigurationRecord) -> bool {
    s.governance.iter().any(|t| {
        (t.r#type == "role_retirement" && t.definition_id == r.definition_id)
            || (t.r#type == "configuration_retirement"
                && s.configuration(&t.definition_id).is_ok_and(|source| {
                    source.r#type == r.r#type && source.definition_id == r.definition_id
                }))
    })
}

/// Model provider sources are pure facts; only the public id must be unique.
pub(crate) fn validate_model_provider(
    s: &EngineeringState,
    data: &Value,
    exclude: Option<&str>,
) -> Result<()> {
    validate_source_data("model_provider", &References::new(), data)?;
    let provider: ProviderFacts = serde_json::from_value(data.clone())
        .map_err(|e| err("invalid_configuration", e.to_string()))?;
    require(
        !s.governance.iter().any(|r| {
            r.r#type == "model_provider"
                && Some(r.definition_id.as_str()) != exclude
                && r.data["id"] == json!(provider.id)
        }),
        "服务商标识已被其他源使用",
    )?;
    Ok(())
}

/// Catalog sources must only reference published model providers.
pub(crate) fn validate_model_catalog(
    s: &EngineeringState,
    data: &Value,
    scope: &Scope,
) -> Result<()> {
    validate_source_data("model_catalog", &References::new(), data)?;
    let catalog: ModelCatalogDefinition = serde_json::from_value(data.clone())
        .map_err(|e| err("invalid_configuration", e.to_string()))?;
    for provider in &catalog.providers {
        reference(s, &provider.provider_ref, "model_provider", scope)?;
    }
    Ok(())
}

/// Harness sources declare their model providers and the catalog verified for their build.
pub(crate) fn validate_harness(
    s: &EngineeringState,
    refs: &References,
    data: &Value,
    scope: &Scope,
) -> Result<()> {
    let h: HarnessDefinition = serde_json::from_value(data.clone())
        .map_err(|e| err("invalid_configuration", e.to_string()))?;
    require(
        h.recipe_digest.as_ref().is_none_or(|digest| {
            known_harness_provider(&h.provider)
                && digest.len() == 64
                && digest.bytes().all(|b| b.is_ascii_hexdigit())
        }),
        "无效的固定 Harness 配方",
    )?;
    require(
        known_harness_provider(&h.provider) && !h.runtime_version.is_empty(),
        "无效 Harness 或运行版本",
    )?;
    validate_source_data("harness", refs, data)?;
    let mut provider_ids = std::collections::BTreeSet::new();
    for provider_ref in &reference_list(refs, "model_providers")? {
        let provider: ProviderFacts =
            reference(s, provider_ref, "model_provider", scope)?.decode()?;
        require(
            provider.harness == h.provider,
            "Harness 引用了不属于它的 Model 服务商",
        )?;
        require(
            provider_ids.insert(provider.id.clone()),
            "Harness 服务商引用重复",
        )?;
    }
    if let Some(catalog_ref) = reference_value(refs, "model_catalog_ref").as_str() {
        let catalog: ModelCatalogDefinition =
            reference(s, catalog_ref, "model_catalog", scope)?.decode()?;
        require(
            h.recipe_digest.as_deref() == Some(catalog.recipe_digest.as_str()),
            "模型目录与 Harness 的构建配方不一致",
        )?;
    }
    Ok(())
}
