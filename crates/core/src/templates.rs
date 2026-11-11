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
struct TemplateEntry {
    template_key: String,
    record: Value,
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
fn load_bundle() -> Result<Bundle> {
    let manifest: Manifest = serde_json::from_str(resource("manifest.json")?)
        .map_err(|e| err("invalid_template", e.to_string()))?;
    require(manifest.schema_version == 2, "只支持源定义协议 v2")?;
    let mut entries = Vec::new();
    let mut keys = BTreeSet::new();
    let mut hashed = BTreeMap::new();
    for (kind, paths) in &manifest.types {
        for path in paths {
            let body = resource(path)?;
            hashed.insert(path.clone(), body.to_owned());
            let mut source: SourceDefinition = serde_json::from_str(body)
                .map_err(|e| err("invalid_template", format!("{path}: {e}")))?;
            require(source.kind == *kind, "资源分类与类型不一致")?;
            // Markdown is owned source text, read once into its owning definition.
            let object = source
                .data
                .as_object_mut()
                .ok_or_else(|| err("invalid_template", "配置正文必须为对象"))?;
            let fields: Vec<_> = object
                .keys()
                .filter(|key| key.ends_with("_file"))
                .cloned()
                .collect();
            for field in fields {
                let filename = object
                    .remove(&field)
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .ok_or_else(|| err("invalid_template", "正文文件引用无效"))?;
                let parent = std::path::Path::new(path).parent().unwrap();
                let file = parent.join(filename);
                let name = file.to_str().unwrap();
                let text = resource(name)?;
                hashed.insert(name.to_owned(), text.to_owned());
                object.insert(field.trim_end_matches("_file").into(), json!(text));
            }
            let record = source.into_record()?;
            require(keys.insert(record.id.clone()), "源定义修订重复")?;
            entries.push(TemplateEntry {
                template_key: record.id.clone(),
                record: json!(record),
            });
        }
    }
    let defaults: Value = serde_json::from_str(resource(&manifest.defaults)?)
        .map_err(|e| err("invalid_template", e.to_string()))?;
    let hash = configuration_hash(
        &json!({"sources":hashed,"defaults":defaults,"codex_recipe":default_codex_recipe_digest(),"dsh_recipe":default_dsh_recipe_digest()}),
    );
    Ok(Bundle {
        manifest,
        entries,
        defaults,
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
fn materialize(
    _generation: Option<u64>,
) -> Result<(EngineeringState, Value, Vec<(String, String)>)> {
    let b = bundle()?;
    let mut s = EngineeringState::default();
    let mut origins = Vec::new();
    for entry in &b.entries {
        let r: ConfigurationRecord = serde_json::from_value(entry.record.clone())
            .map_err(|e| err("invalid_template", e.to_string()))?;
        origins.push((r.id.clone(), entry.template_key.clone()));
        s.governance.push(r);
    }
    s.refresh_configuration_views()?;
    s.validate_governance(&EngineeringState::default())?;
    for r in &s.governance {
        match r.kind.as_str() {
            "model" => validate_model_definition(&r.decode()?)?,
            "capability" => configuration::validate_capability(&s, &r.data, &r.scope)?,
            "agent" => governance::validate_composition(&s, &s.resolve_agent(&r.id)?, &r.scope)?,
            "workflow" => {
                r.decode::<WorkflowDefinition>()?.validate()?;
            }
            "delivery_type" => r.decode::<DeliveryTypeDefinition>()?.validate()?,
            _ => {}
        }
    }
    Ok((s, b.defaults.clone(), origins))
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
    if !template_definition_kind(&r.kind)
        || template_catalog_member(s, r).is_some_and(|m| m.data["generation"] == generation)
    {
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
        configuration_hash(&(template_member_kind(&r.kind), &r.id, generation))
    );
    s.governance.push(member);
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
    for source in next.governance {
        if let Some(existing) = s.governance.iter().find(|r| r.id == source.id) {
            require(existing.source() == source.source(), "同一源修订内容冲突")?;
        } else {
            s.governance.push(source);
        }
    }
    s.refresh_configuration_views()?;
    // Reset changes catalog selection, never source identities.

    for (id, key) in origins {
        let r = s.configuration(&id)?.clone();
        add_member(s, &r, generation, Some((&key, &template_source_hash(&r))));
    }

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
    template_catalog_state(s).map(|r| r.data["defaults"].clone())
}
