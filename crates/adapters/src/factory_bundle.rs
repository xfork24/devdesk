//! Packaged resources are an adapter concern; application validation remains in core.
use super::*;
use std::collections::{BTreeMap, BTreeSet};
include!(concat!(env!("OUT_DIR"), "/template_files.rs"));
fn resource(path: &str) -> Result<&'static str> {
    require(
        !path.is_empty()
            && Path::new(path)
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
pub fn initialize() -> Result<()> {
    static READY: std::sync::OnceLock<Result<()>> = std::sync::OnceLock::new();
    READY
        .get_or_init(|| {
            devdesk_core::install_driver_capabilities(harness::driver_capability_registry())?;
            load()
        })
        .clone()
}
fn load() -> Result<()> {
    let manifest: Value = serde_json::from_str(resource("manifest.json")?).map_err(storage)?;
    let types: BTreeMap<String, Vec<String>> =
        serde_json::from_value(manifest["types"].clone()).map_err(storage)?;
    let mut entries = Vec::new();
    let mut keys = BTreeSet::new();
    let mut hashed = BTreeMap::new();
    for (kind, paths) in &types {
        for path in paths {
            let body = resource(path)?;
            hashed.insert(path.clone(), body.to_owned());
            let mut source: SourceDefinition = serde_json::from_str(body).map_err(storage)?;
            require(source.r#type == *kind, "资源分类与类型不一致")?;
            let object = source
                .data
                .as_object_mut()
                .ok_or_else(|| err("invalid_template", "配置正文必须为对象"))?;
            let fields: Vec<_> = object
                .keys()
                .filter(|k| k.ends_with("_file"))
                .cloned()
                .collect();
            for field in fields {
                let filename = object
                    .remove(&field)
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .ok_or_else(|| err("invalid_template", "正文文件引用无效"))?;
                let file = Path::new(path).parent().unwrap().join(filename);
                let name = file
                    .to_str()
                    .ok_or_else(|| err("invalid_template", "无效资源路径"))?;
                let text = resource(name)?;
                hashed.insert(name.to_owned(), text.to_owned());
                object.insert(field.trim_end_matches("_file").into(), json!(text));
            }
            let record = source.into_record()?;
            require(keys.insert(record.id.clone()), "源定义修订重复")?;
            entries.push(record);
        }
    }
    let defaults: Value = serde_json::from_str(resource(
        manifest["defaults"]
            .as_str()
            .ok_or_else(|| err("invalid_template", "默认清单缺失"))?,
    )?)
    .map_err(storage)?;
    let hash = configuration_hash(&json!({"sources":hashed,"defaults":defaults}));
    devdesk_core::install_configuration_bundle(devdesk_core::ConfigurationBundle {
        bundle_version: manifest["bundle_version"]
            .as_str()
            .ok_or_else(|| err("invalid_template", "缺少包标签"))?
            .into(),
        types,
        entries,
        defaults,
        hash,
    })
}
/// Packaged defaults are ordinary source data, also usable before a dataset is initialized.
pub(crate) fn packaged_runtime(provider: HarnessProvider) -> Result<SourceDefinition> {
    let manifest: Value = serde_json::from_str(resource("manifest.json")?).map_err(storage)?;
    let defaults: Value =
        serde_json::from_str(resource(manifest["defaults"].as_str().unwrap())?).map_err(storage)?;
    let reference = defaults["runtime_resources"][provider.as_str()]
        .as_str()
        .ok_or_else(|| err("configuration_required", "没有默认运行资源选择"))?;
    for path in manifest["types"]["runtime_resource"]
        .as_array()
        .into_iter()
        .flatten()
    {
        let source: SourceDefinition =
            serde_json::from_str(resource(path.as_str().unwrap())?).map_err(storage)?;
        if source_reference(&source.identity, source.revision)? == reference {
            return Ok(source);
        }
    }
    Err(err("configuration_required", "默认运行资源不存在"))
}
