//! Embedded factory definitions, database copies, and append-only catalog generations.
#[cfg(test)]
#[path = "../../../tests/support/defaults.rs"]
mod default_fixture;
use crate::governance::{record, update_record};
use crate::*;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

include!(concat!(env!("OUT_DIR"), "/template_files.rs"));

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema_version: u32,
    bundle_version: String,
    types: BTreeMap<String, Vec<String>>,
    defaults: String,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct TemplateReference {
    template_ref: String,
    target: String,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct TemplateEntry {
    template_key: String,
    storage: String,
    record: Value,
    text_files: BTreeMap<String, String>,
    references: BTreeMap<String, TemplateReference>,
    initial_only: bool,
}
struct Bundle {
    manifest: Manifest,
    entries: Vec<TemplateEntry>,
    defaults: Value,
    hash: String,
}
fn resource(path: &str) -> Result<&'static str> {
    require(
        !path.is_empty()
            && std::path::Path::new(path)
                .components()
                .all(|p| matches!(p, std::path::Component::Normal(_))),
        "模板资源路径越界",
    )?;
    TEMPLATE_FILES
        .iter()
        .find(|(p, _)| *p == path)
        .map(|(_, v)| *v)
        .ok_or_else(|| err("invalid_template", format!("缺少模板文件 {path}")))
}
fn decode<T: serde::de::DeserializeOwned>(v: Value) -> Result<T> {
    serde_json::from_value(v).map_err(|e| err("invalid_template", e.to_string()))
}
fn reference_pointer(pointer: &str) -> bool {
    let parts: Vec<_> = pointer.split('/').collect();
    if parts.iter().any(|p| {
        matches!(
            *p,
            "parameters"
                | "input_schema"
                | "output_schema"
                | "instructions"
                | "body"
                | "task_instructions"
        )
    }) {
        return false;
    }
    let key = parts.last().copied().unwrap_or("");
    matches!(
        key,
        "integration_ref"
            | "role"
            | "tool_ref"
            | "model_ref"
            | "harness_ref"
            | "capability_ref"
            | "role_ref"
            | "default_role_ref"
            | "delivery_ref"
            | "workflow_ref"
            | "standard_ref"
            | "sandbox_ref"
            | "revision_id"
            | "agent_revision_id"
            | "agent_ref"
            | "definition_id"
            | "supersedes"
    ) || (matches!(key, "id" | "identity") && pointer.starts_with("/data/"))
        || parts.iter().any(|p| {
            matches!(
                *p,
                "tools"
                    | "skills"
                    | "permissions"
                    | "skill_revisions"
                    | "tool_revisions"
                    | "spec_refs"
                    | "required_skill_refs"
                    | "tool_refs"
                    | "required_spec_refs"
                    | "allowed_tools"
                    | "requires_tools"
            )
        })
}
fn load_bundle() -> Result<Bundle> {
    let manifest: Manifest = serde_json::from_str(resource("manifest.json")?)
        .map_err(|e| err("invalid_template", e.to_string()))?;
    require(
        manifest.schema_version == 1 && !manifest.bundle_version.is_empty(),
        "模板清单版本不支持",
    )?;
    let mut entries = Vec::new();
    let mut keys = BTreeSet::new();
    let mut files = BTreeSet::new();
    let mut hashed = BTreeMap::from([("manifest.json".to_owned(), resource("manifest.json")?)]);
    for paths in manifest.types.values() {
        for path in paths {
            require(files.insert(path), "模板文件重复")?;
            let body = resource(path)?;
            hashed.insert(path.clone(), body);
            let mut entry: TemplateEntry = serde_json::from_str(body)
                .map_err(|e| err("invalid_template", format!("{path}: {e}")))?;
            require(keys.insert(entry.template_key.clone()), "模板 key 重复")?;
            require(
                matches!(
                    entry.storage.as_str(),
                    "governance" | "definitions" | "harnesses" | "sandboxes"
                ),
                "模板存储类型错误",
            )?;
            for (pointer, path) in &entry.text_files {
                let body = resource(path)?;
                require(
                    matches!(
                        pointer.rsplit('/').next(),
                        Some(
                            "instructions" | "body" | "task_instructions" | "goal" | "description"
                        )
                    ),
                    "正文引用位置不支持",
                )?;
                let field = entry
                    .record
                    .pointer_mut(pointer)
                    .ok_or_else(|| err("invalid_template", "模板正文位置不存在"))?;
                require(field.is_string(), "模板正文必须是字符串")?;
                *field = json!(body);
                hashed.insert(path.clone(), body);
            }
            for (pointer, reference) in &entry.references {
                require(
                    reference_pointer(pointer) && entry.record.pointer(pointer).is_some(),
                    format!("不支持的模板引用位置 {pointer}"),
                )?;
                require(
                    matches!(reference.target.as_str(), "revision" | "identity"),
                    "模板引用目标类型错误",
                )?;
            }
            entries.push(entry);
        }
    }
    for entry in &entries {
        for reference in entry.references.values() {
            require(
                keys.contains(&reference.template_ref),
                format!("模板依赖缺失 {}", reference.template_ref),
            )?;
        }
    }
    let defaults = resource(&manifest.defaults)?;
    hashed.insert(manifest.defaults.clone(), defaults);
    let hash = configuration_hash(
        &json!({"files":hashed,"codex_recipe":default_codex_recipe_digest(),"dsh_recipe":default_dsh_recipe_digest()}),
    );
    Ok(Bundle {
        manifest,
        entries,
        defaults: serde_json::from_str(defaults)
            .map_err(|e| err("invalid_template", e.to_string()))?,
        hash,
    })
}
fn bundle() -> Result<&'static Bundle> {
    static BUNDLE: OnceLock<Result<Bundle>> = OnceLock::new();
    BUNDLE
        .get_or_init(load_bundle)
        .as_ref()
        .map_err(Clone::clone)
}
pub fn template_bundle_info() -> Result<Value> {
    let b = bundle()?;
    Ok(
        json!({"bundle_version":b.manifest.bundle_version,"bundle_hash":b.hash,"types":b.manifest.types.iter().map(|(k,v)|(k.clone(),json!(v.len()))).collect::<serde_json::Map<String,Value>>()}),
    )
}
fn remapped(value: &str, generation: Option<u64>, identity: bool) -> String {
    generation
        .map(|g| {
            format!(
                "template-{}:{g}:{}",
                if identity { "identity" } else { "revision" },
                &configuration_hash(&value)[..24]
            )
        })
        .unwrap_or_else(|| value.into())
}
fn materialize(
    generation: Option<u64>,
) -> Result<(EngineeringState, Value, Vec<(String, String)>)> {
    let b = bundle()?;
    let by_key: BTreeMap<_, _> = b
        .entries
        .iter()
        .map(|e| (e.template_key.as_str(), e))
        .collect();
    let mut tables: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    let mut origins = Vec::new();
    let resolve = |key: &str, identity: bool| -> Result<String> {
        let entry = by_key
            .get(key)
            .ok_or_else(|| err("invalid_template", format!("模板不存在 {key}")))?;
        let field = if identity {
            if entry.storage == "governance" {
                "definition_id"
            } else {
                "identity"
            }
        } else {
            "id"
        };
        let source = entry.record[field]
            .as_str()
            .ok_or_else(|| err("invalid_template", "引用目标没有指定身份"))?;
        Ok(remapped(source, generation, identity))
    };
    for entry in &b.entries {
        if generation.is_some() && entry.initial_only {
            continue;
        }
        let mut value = entry.record.clone();
        value["id"] = json!(resolve(&entry.template_key, false)?);
        for field in ["definition_id", "identity"] {
            if let Some(source) = entry.record[field].as_str() {
                value[field] = json!(remapped(source, generation, true));
            }
        }
        for (pointer, reference) in &entry.references {
            *value.pointer_mut(pointer).unwrap() = json!(resolve(
                &reference.template_ref,
                reference.target == "identity"
            )?);
        }
        if entry.storage == "governance" {
            value["content_hash"] = json!(configuration_hash(&value["data"]));
            if generation.is_some() {
                value["created_at"] = json!(now());
            }
            origins.push((
                value["id"].as_str().unwrap().into(),
                entry.template_key.clone(),
            ));
        }
        tables.entry(entry.storage.clone()).or_default().push(value);
    }
    let mut full = json!(EngineeringState::default());
    for (table, rows) in tables {
        full[table] = json!(rows);
    }
    let mut s: EngineeringState = decode(full)?;
    s.validate_governance(&EngineeringState::default())?;
    for r in &s.governance {
        match r.kind.as_str() {
            "model" => validate_model_definition(&r.decode()?)?,
            "role" => {
                let role: RoleDefinition = r.decode()?;
                if let Some(v) = role.visibility {
                    v.validate()?;
                }
            }
            "capability" => configuration::validate_capability(&s, &r.data, &r.scope)?,
            "agent" => governance::validate_composition(&s, &r.decode()?, &r.scope)
                .map_err(|e| err(&e.code, format!("{}: {}", r.name, e.message)))?,
            "workflow" => {
                let doc: WorkflowDefinition = r.decode()?;
                doc.validate()?;
            }
            "delivery_type" => r.decode::<DeliveryTypeDefinition>()?.validate()?,
            "integration" => r.decode::<IntegrationDefinition>()?.validate()?,
            "integration_binding" => {
                let binding: IntegrationBinding = r.decode()?;
                binding.validate_origins()?;
                require(
                    binding.auth.credential_ref().is_none(),
                    "默认接入绑定不能包含机器凭据引用",
                )?;
                governance::reference(&s, &binding.integration_ref, "integration", &r.scope)?;
                governance::reference(&s, &binding.harness_ref, "harness", &r.scope)?;
                require(
                    s.governance
                        .iter()
                        .any(|v| v.kind == "sandbox" && v.id == binding.sandbox_ref)
                        || s.sandboxes.iter().any(|v| v.id == binding.sandbox_ref),
                    "默认接入 Sandbox 缺失",
                )?;
                require(
                    (100..=120_000).contains(&binding.timeout_ms)
                        && (1024..=8_388_608).contains(&binding.max_result_bytes),
                    "默认接入调用限制无效",
                )?;
                for tool in &binding.allowed_tools {
                    let tool = governance::reference(&s, tool, "tool", &r.scope)?;
                    require(
                        tool.data["integration_ref"] == binding.integration_ref,
                        "默认接入工具来源不一致",
                    )?;
                }
            }
            "tool" if r.data.get("integration_ref").is_some() => {
                let tool: ExternalTool = r.decode()?;
                tool.validate()?;
                governance::reference(&s, &tool.integration_ref, "integration", &r.scope)?;
            }
            "tool" => {
                let tool: ToolDefinition = r.decode()?;
                require(
                    !tool.qualified_name.is_empty()
                        && (tool.implementation == "harness.native"
                            || (tool.implementation == "platform"
                                && crate::platform_tools::PLATFORM_HANDLERS
                                    .contains(&tool.qualified_name.as_str())))
                        && tool.input_schema.is_object()
                        && tool.output_schema.is_object(),
                    "默认原生工具契约无效",
                )?;
            }
            "harness" => {
                let harness: HarnessDefinition = r.decode()?;
                if let Some(digest) = harness.recipe_digest {
                    let expected = match harness.provider.as_str() {
                        "codex" => default_codex_recipe_digest(),
                        "dsh" => default_dsh_recipe_digest(),
                        _ => return Err(err("invalid_template", "默认 Harness provider 不支持")),
                    };
                    require(digest == expected, "默认 Harness 配方摘要与嵌入资产不一致")?;
                }
            }
            "permission_profile" => {
                let mut profile: PermissionProfile = r.decode()?;
                profile.normalize()?;
            }
            _ => {}
        }
    }
    let mut defaults = b.defaults.clone();
    for key in [
        "workflow_ref",
        "task_workflow_ref",
        "model_ref",
        "harness_ref",
        "sandbox_ref",
        "capability_ref",
        "permission_sandbox_ref",
        "harness_permission_ref",
        "sandbox_permission_ref",
    ] {
        defaults[key] = json!(resolve(text(&b.defaults, key)?.as_str(), false)?);
    }
    defaults["role_agent_bindings"] = json!(
        b.defaults["role_agent_bindings"]
            .as_object()
            .ok_or_else(|| err("invalid_template", "默认角色绑定缺失"))?
            .iter()
            .map(|(role, agent)| Ok((
                resolve(role, true)?,
                json!(resolve(agent.as_str().unwrap_or(""), false)?)
            )))
            .collect::<Result<serde_json::Map<String, Value>>>()?
    );
    // Projections are loaded from the same resources and keep their precise revision IDs.
    s.events.clear();
    Ok((s, defaults, origins))
}
/// Counts published logical definitions, rather than source files or historical revisions.
pub fn template_factory_counts() -> Result<BTreeMap<String, usize>> {
    static COUNTS: OnceLock<Result<BTreeMap<String, usize>>> = OnceLock::new();
    COUNTS
        .get_or_init(|| materialize(None).map(|(s, _, _)| template_catalog_counts(&s)))
        .clone()
}

fn add_member(
    s: &mut EngineeringState,
    r: &ConfigurationRecord,
    generation: u64,
    source: Option<(&str, &str)>,
) {
    if !template_definition_kind(&r.kind) || template_catalog_member(s, r).is_some() {
        return;
    }
    let inherited = r
        .supersedes
        .as_deref()
        .and_then(|id| s.configuration(id).ok())
        .and_then(|old| template_catalog_member(s, old))
        .map(|m| m.data.clone());
    let template_key = source
        .map(|s| json!(s.0))
        .or_else(|| inherited.as_ref().map(|m| m["template_key"].clone()))
        .unwrap_or(Value::Null);
    let source_hash = source
        .map(|s| json!(s.1))
        .or_else(|| inherited.as_ref().map(|m| m["source_hash"].clone()))
        .unwrap_or(Value::Null);
    let data = json!({"kind":template_member_kind(&r.kind),"generation":generation,"template_key":template_key,"source_hash":source_hash,"origin":if template_key.is_string(){"default"}else{"custom"}});
    let mut member = record(
        "template_catalog_member",
        r.id.clone(),
        r.scope.clone(),
        r.name.clone(),
        1,
        None,
        data,
        true,
    );
    member.created_at = 0;
    member.id = format!(
        "template-member:{}",
        configuration_hash(&(template_member_kind(&r.kind), &r.id))
    );
    s.governance.push(member);
}
fn add_projection_members(
    s: &mut EngineeringState,
    generation: u64,
    source_generation: Option<Option<u64>>,
) -> Result<()> {
    let definitions = s.definitions.clone();
    for definition in definitions {
        if s.governance
            .iter()
            .any(|r| r.id == definition.id && r.is_definition())
            || s.governance
                .iter()
                .any(|r| r.kind == "template_catalog_member" && r.definition_id == definition.id)
        {
            continue;
        }
        let source = if let Some(allocation) = source_generation {
            bundle()?.entries.iter().find(|e| {
                e.storage == "definitions"
                    && e.record["id"]
                        .as_str()
                        .is_some_and(|id| remapped(id, allocation, false) == definition.id)
            })
        } else {
            None
        };
        let source =
            source.filter(|e| source_generation != Some(None) || e.record == json!(definition));
        let data = json!({"kind":format!("definition:{}",definition.family),"generation":generation,"template_key":source.map(|e|&e.template_key),"source_hash":source.map(|_|configuration_hash(&definition)),"origin":if source.is_some(){"default"}else if source_generation.is_some(){"existing"}else{"custom"}});
        let mut member = record(
            "template_catalog_member",
            definition.id.clone(),
            Scope::Global,
            definition.name.clone(),
            1,
            None,
            data,
            true,
        );
        member.created_at = 0;
        member.id = format!(
            "template-member:{}",
            configuration_hash(&(format!("definition:{}", definition.family), &definition.id))
        );
        s.governance.push(member);
    }
    Ok(())
}
pub fn register_template_members(s: &mut EngineeringState, generation: u64) {
    let records: Vec<_> = s
        .governance
        .iter()
        .filter(|r| template_definition_kind(&r.kind) && template_catalog_member(s, r).is_none())
        .cloned()
        .collect();
    for r in records {
        add_member(s, &r, generation, None);
    }
    add_projection_members(s, generation, None).expect("projection membership has no I/O");
}
fn register_existing_members(s: &mut EngineeringState) {
    let before: BTreeSet<_> = s
        .governance
        .iter()
        .filter(|r| r.kind == "template_catalog_member")
        .map(|r| r.id.clone())
        .collect();
    register_template_members(s, TEMPLATE_INITIAL_GENERATION);
    for member in s.governance.iter_mut().filter(|r| {
        r.kind == "template_catalog_member"
            && !before.contains(&r.id)
            && r.data["template_key"].is_null()
    }) {
        member.data["origin"] = json!("existing");
        member.content_hash = configuration_hash(&member.data);
    }
}

fn set_state(s: &mut EngineeringState, generation: u64, defaults: Value) -> Result<()> {
    let b = bundle()?;
    let data = json!({"generation":generation,"initial_generation":TEMPLATE_INITIAL_GENERATION,"bundle_version":b.manifest.bundle_version,"bundle_hash":b.hash,"defaults":defaults});
    if let Some(state) = s
        .governance
        .iter_mut()
        .find(|r| r.id == TEMPLATE_CATALOG_STATE)
    {
        update_record(state, data);
    } else {
        let mut state = record(
            "template_catalog_state",
            TEMPLATE_CATALOG_STATE.into(),
            Scope::Global,
            "模板定义目录".into(),
            1,
            None,
            data,
            false,
        );
        state.id = TEMPLATE_CATALOG_STATE.into();
        s.governance.push(state);
    }
    Ok(())
}
pub fn initialize_template_catalog(s: &mut EngineeringState) -> Result<()> {
    if template_catalog_state(s).is_some() {
        return Ok(());
    }
    let (factory, mut defaults, origins) = materialize(None)?;
    let empty = s.governance.is_empty()
        && s.definitions.is_empty()
        && s.harnesses.is_empty()
        && s.sandboxes.is_empty();
    if !empty {
        let resolve = |key: &str| -> String {
            let source = factory.governance.iter().find(|r| r.id == key);
            source
                .and_then(|source| {
                    s.governance
                        .iter()
                        .filter(|r| {
                            r.kind == source.kind && r.definition_id == source.definition_id
                        })
                        .max_by_key(|r| r.version)
                })
                .map(|r| r.id.clone())
                .unwrap_or_else(|| key.into())
        };
        for key in [
            "workflow_ref",
            "task_workflow_ref",
            "model_ref",
            "harness_ref",
            "capability_ref",
        ] {
            if let Some(id) = defaults[key].as_str() {
                defaults[key] = json!(resolve(id));
            }
        }
        if let Some(bindings) = defaults["role_agent_bindings"].as_object_mut() {
            for value in bindings.values_mut() {
                if let Some(id) = value.as_str() {
                    *value = json!(resolve(id));
                }
            }
        }
    }
    if empty {
        s.governance = factory.governance;
        s.harnesses = factory.harnesses;
        s.sandboxes = factory.sandboxes;
        s.definitions = factory.definitions;
    }
    let b = bundle()?;
    // Known initial IDs are annotated only when the complete record is still identical.
    for (id, key) in origins {
        let Some(r) = s.governance.iter().find(|r| r.id == id).cloned() else {
            continue;
        };
        let source = b.entries.iter().find(|e| e.template_key == key).unwrap();
        let is_source = r.data == source.record["data"]
            && json!(r.name) == source.record["name"]
            && json!(r.scope) == source.record["scope"];
        add_member(
            s,
            &r,
            TEMPLATE_INITIAL_GENERATION,
            is_source.then_some((key.as_str(), template_source_hash(&r).as_str())),
        );
    }
    add_projection_members(s, TEMPLATE_INITIAL_GENERATION, Some(None))?;
    if empty {
        register_template_members(s, TEMPLATE_INITIAL_GENERATION);
    } else {
        register_existing_members(s);
    }
    set_state(s, TEMPLATE_INITIAL_GENERATION, defaults)
}
pub fn copy_template_catalog(
    source: &EngineeringState,
    target: &mut EngineeringState,
) -> Result<()> {
    // Register existing local resources in the fixed initial generation, including unopened projects.
    for member in source
        .governance
        .iter()
        .filter(|r| r.kind == "template_catalog_member")
    {
        if !target.governance.iter().any(|r| r.id == member.id) {
            target.governance.push(member.clone());
        }
    }
    register_existing_members(target);
    if template_catalog_generation(target) > template_catalog_generation(source) {
        return Ok(());
    }
    if let Some(state) = template_catalog_state(source) {
        if let Some(old) = target.governance.iter_mut().find(|r| r.id == state.id) {
            *old = state.clone();
        } else {
            target.governance.push(state.clone());
        }
    }
    Ok(())
}
pub fn restore_template_catalog(s: &mut EngineeringState, command_id: &str) -> Result<Value> {
    let old = template_catalog_generation(s);
    let generation = old
        .checked_add(1)
        .ok_or_else(|| err("invalid_configuration", "目录代次溢出"))?;
    let (next, defaults, origins) = materialize(Some(generation))?;
    let b = bundle()?;
    s.harnesses.extend(next.harnesses);
    s.sandboxes.extend(next.sandboxes);
    s.definitions.extend(next.definitions);
    s.governance.extend(next.governance);
    for (id, key) in origins {
        let r = s.configuration(&id)?.clone();
        add_member(s, &r, generation, Some((&key, &template_source_hash(&r))));
    }
    add_projection_members(s, generation, Some(Some(generation)))?;
    set_state(s, generation, defaults)?;
    let counts = template_catalog_counts(s);
    let result = json!({"generation":generation,"previous_generation":old,"bundle_version":b.manifest.bundle_version,"bundle_hash":b.hash,"counts":counts,"changed":true,"receipt_id":format!("template-reset:{command_id}")});
    let mut receipt = record(
        "template_reset_receipt",
        command_id.into(),
        Scope::Global,
        "重置模板定义".into(),
        1,
        None,
        result.clone(),
        true,
    );
    receipt.id = format!("template-reset:{command_id}");
    s.governance.push(receipt);
    Ok(result)
}
pub fn template_catalog_counts(s: &EngineeringState) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for r in s.governance.iter().filter(|r| {
        template_definition_kind(&r.kind)
            && template_record_current(s, r)
            && !configuration::retired(s, r)
    }) {
        if !s.governance.iter().any(|n| {
            n.kind == r.kind && n.definition_id == r.definition_id && n.version > r.version
        }) {
            *counts.entry(r.kind.clone()).or_default() += 1;
        }
    }
    counts
}
pub fn template_catalog_pristine(s: &EngineeringState) -> bool {
    if template_catalog_state(s).is_none_or(|r| {
        r.data["bundle_hash"] != bundle().map(|b| json!(b.hash)).unwrap_or(Value::Null)
    }) {
        return false;
    }
    if s.definitions
        .iter()
        .filter(|d| template_projection_current(s, d))
        .any(|d| {
            !s.governance
                .iter()
                .any(|r| r.id == d.id && r.is_definition())
                && s.governance
                    .iter()
                    .find(|m| m.kind == "template_catalog_member" && m.definition_id == d.id)
                    .is_none_or(|m| m.data["source_hash"] != configuration_hash(d))
        })
    {
        return false;
    }
    s.governance
        .iter()
        .filter(|r| template_definition_kind(&r.kind) && template_record_current(s, r))
        .filter(|r| r.kind != "workflow_draft" || r.data["published_revision_id"].is_null())
        .filter(|r| {
            !s.governance.iter().any(|n| {
                n.kind == r.kind && n.definition_id == r.definition_id && n.version > r.version
            })
        })
        .all(|r| {
            r.kind != "workflow_draft"
                && !configuration::retired(s, r)
                && !template_record_disabled(s, r)
                && template_catalog_member(s, r)
                    .is_some_and(|m| m.data["source_hash"] == template_source_hash(r))
        })
}
pub fn pinned_template_records(
    s: &EngineeringState,
    configuration: Option<&Value>,
) -> Vec<ConfigurationRecord> {
    fn collect(value: &Value, ids: &mut BTreeSet<String>) {
        match value {
            Value::String(v) => {
                ids.insert(v.clone());
            }
            Value::Array(v) => {
                for v in v {
                    collect(v, ids);
                }
            }
            Value::Object(v) => {
                for (k, v) in v {
                    ids.insert(k.clone());
                    collect(v, ids);
                }
            }
            _ => {}
        }
    }
    let mut ids = BTreeSet::new();
    if let Some(data) = configuration {
        collect(data, &mut ids);
    }
    loop {
        let before = ids.len();
        for r in &s.governance {
            if ids.contains(&r.id) || ids.contains(&r.definition_id) {
                collect(&r.data, &mut ids);
            }
        }
        if before == ids.len() {
            break;
        }
    }
    s.governance
        .iter()
        .filter(|r| r.is_definition() && (ids.contains(&r.id) || ids.contains(&r.definition_id)))
        .cloned()
        .collect()
}

pub fn current_template_defaults(s: &EngineeringState) -> Option<Value> {
    let mut defaults = template_catalog_state(s)?.data["defaults"].clone();
    let latest = |id: &str| {
        s.configuration(id)
            .ok()
            .and_then(|source| {
                s.governance
                    .iter()
                    .filter(|r| {
                        r.kind == source.kind
                            && r.definition_id == source.definition_id
                            && r.immutable
                            && template_record_current(s, r)
                    })
                    .max_by_key(|r| r.version)
            })
            .map(|r| r.id.clone())
            .unwrap_or_else(|| id.to_owned())
    };
    for key in [
        "workflow_ref",
        "task_workflow_ref",
        "model_ref",
        "harness_ref",
        "capability_ref",
        "permission_sandbox_ref",
        "harness_permission_ref",
        "sandbox_permission_ref",
    ] {
        if let Some(id) = defaults[key].as_str() {
            defaults[key] = json!(latest(id));
        }
    }
    if let Some(bindings) = defaults["role_agent_bindings"].as_object_mut() {
        for id in bindings.values_mut() {
            if let Some(value) = id.as_str() {
                *id = json!(latest(value));
            }
        }
    }
    Some(defaults)
}

#[cfg(test)]
mod tests {
    use super::default_fixture::*;
    use super::*;
    #[test]
    fn resources_restore_fresh_references_and_keep_the_original_graph() {
        let mut s = EngineeringState::default();
        initialize_template_catalog(&mut s).unwrap();
        let before = s.clone();
        let original = s.configuration(DEFAULT_MODEL_ID).unwrap().clone();
        restore_template_catalog(&mut s, "reset-one").unwrap();
        assert_eq!(s.configuration(&original.id).unwrap(), &original);
        assert!(!template_record_current(&s, &original));
        let refs = current_template_defaults(&s).unwrap();
        let wf = s
            .configuration(refs["workflow_ref"].as_str().unwrap())
            .unwrap();
        wf.decode::<WorkflowDefinition>()
            .unwrap()
            .validate()
            .unwrap();
        assert!(template_catalog_pristine(&s));
        s.validate_governance(&before).unwrap();
        assert_ne!(refs["model_ref"], DEFAULT_MODEL_ID);
        let mut project = s.clone();
        copy_template_catalog(&before, &mut project).unwrap();
        assert_eq!(template_catalog_generation(&project), 2);
        assert!(
            project
                .definitions
                .iter()
                .filter(|d| template_projection_current(&project, d))
                .all(|d| d.id.starts_with("template-revision:2:"))
        );
    }
    #[test]
    fn sandbox_only_existing_data_is_preserved_during_catalog_registration() {
        let mut factory = EngineeringState::default();
        initialize_template_catalog(&mut factory).unwrap();
        let mut custom = factory.sandboxes[0].clone();
        custom.id = "custom-sandbox".into();
        let mut existing = EngineeringState {
            sandboxes: vec![custom.clone()],
            ..Default::default()
        };
        initialize_template_catalog(&mut existing).unwrap();
        assert_eq!(json!(existing.sandboxes), json!([custom]));
        assert!(existing.definitions.is_empty());
    }
    #[test]
    fn missing_files_and_arbitrary_reference_locations_are_rejected() {
        assert!(resource("../manifest.json").is_err());
        assert!(resource("missing.json").is_err());
        assert!(!reference_pointer("/data/instructions"));
        assert!(!reference_pointer("/data/parameters/message"));
    }
}
