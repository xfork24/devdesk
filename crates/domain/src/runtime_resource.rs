//! Configurable immutable runtime inputs; labels never determine protocol compatibility.
use crate::*;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeResourceFile {
    pub path: String,
    pub content: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeRuntimeRelease {
    pub url: String,
    pub sha256: String,
    pub member: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeResourceDefinition {
    pub provider: HarnessProvider,
    pub version: String,
    pub license: String,
    #[serde(default)]
    pub files: Vec<RuntimeResourceFile>,
    #[serde(default)]
    pub image: Option<String>,
    #[serde(default)]
    pub native_releases: BTreeMap<String, NativeRuntimeRelease>,
}
pub fn runtime_resource_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 1024
        && !path.contains('\\')
        && !path.contains('\0')
        && path
            .split('/')
            .all(|s| !s.is_empty() && s != "." && s != "..")
}
impl RuntimeResourceDefinition {
    pub fn validate(&self) -> Result<()> {
        require(
            !self.version.is_empty()
                && self.version.len() <= 160
                && self
                    .version
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "._+-".contains(c)),
            "运行资源标签格式无效",
        )?;
        require(!self.license.is_empty(), "运行资源需要许可证/来源说明")?;
        if let Some(image) = &self.image {
            require(
                self.files.is_empty()
                    && image.split_once("@sha256:").is_some_and(|(name, digest)| {
                        !name.is_empty()
                            && digest.len() == 64
                            && digest.bytes().all(|b| b.is_ascii_hexdigit())
                    }),
                "镜像必须使用完整不可变摘要，不能同时指定构建文件",
            )?;
        } else {
            require(
                !self.files.is_empty() && self.files.len() <= 128,
                "构建配方需要 1—128 个文件",
            )?;
            let mut paths = BTreeSet::new();
            let mut total = 0usize;
            for file in &self.files {
                require(
                    runtime_resource_path(&file.path) && paths.insert(&file.path),
                    "构建文件路径越界或重复",
                )?;
                total = total.saturating_add(file.content.len());
            }
            require(
                paths.contains(&"Dockerfile".to_owned()) && total <= 8 * 1024 * 1024,
                "构建配方必须包含 Dockerfile 且不超过 8 MiB",
            )?;
        }
        for release in self.native_releases.values() {
            let url = url::Url::parse(&release.url)
                .map_err(|_| err("invalid_source", "运行下载 URL 无效"))?;
            require(
                url.scheme() == "https"
                    && url.username().is_empty()
                    && url.password().is_none()
                    && url.query().is_none()
                    && url.fragment().is_none(),
                "运行下载使用无内嵌凭据的 HTTPS 地址",
            )?;
            require(
                release.sha256.len() == 64
                    && release.sha256.bytes().all(|b| b.is_ascii_hexdigit())
                    && runtime_resource_path(&release.member),
                "运行下载需要完整摘要和安全成员路径",
            )?;
        }
        Ok(())
    }
    pub fn recipe_digest(&self) -> String {
        if let Some(image) = &self.image {
            configuration_hash(&json!({"image":image}))
        } else {
            configuration_hash(
                &self
                    .files
                    .iter()
                    .map(|f| (&f.path, &f.content))
                    .collect::<Vec<_>>(),
            )
        }
    }
}
/// Resolve either an explicit source revision or a typed content-address dependency.
pub fn resolve_configuration_edge<'a>(
    state: &'a EngineeringState,
    edge: &ConfigurationEdge,
) -> Result<&'a ConfigurationRecord> {
    if edge.relation == "content_digest" {
        state
            .governance
            .iter()
            .find(|r| {
                r.r#type == edge.expected_type
                    && state.configuration(&edge.from).is_ok_and(|origin| {
                        r.scope.visible_from(&origin.scope)
                            && r.data["provider"] == origin.data["provider"]
                    })
                    && r.decode::<RuntimeResourceDefinition>()
                        .is_ok_and(|resource| resource.recipe_digest() == edge.to)
            })
            .ok_or_else(|| {
                err(
                    "configuration_required",
                    format!("未找到运行资源配方 {}", edge.to),
                )
            })
    } else {
        state.configuration(&edge.to)
    }
}
pub fn harness_runtime_resource<'a>(
    state: &'a EngineeringState,
    harness: &ConfigurationRecord,
) -> Result<&'a ConfigurationRecord> {
    let edge = configuration_edges(harness)?
        .into_iter()
        .find(|e| e.expected_type == "runtime_resource")
        .ok_or_else(|| {
            err(
                "configuration_required",
                "Harness 需要明确运行资源引用或配方摘要",
            )
        })?;
    let source = resolve_configuration_edge(state, &edge)?;
    require(
        source.r#type == "runtime_resource" && source.scope.visible_from(&harness.scope),
        "运行资源类型或范围不匹配",
    )?;
    let resource: RuntimeResourceDefinition = source.decode()?;
    resource.validate()?;
    let definition: HarnessDefinition = harness.decode()?;
    require(
        resource.provider.as_str() == definition.provider,
        "运行资源与 Harness 协议驱动不一致",
    )?;
    require(
        definition
            .recipe_digest
            .as_ref()
            .is_none_or(|digest| *digest == resource.recipe_digest()),
        "Harness 引用与配方摘要不一致",
    )?;
    Ok(source)
}
