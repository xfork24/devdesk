//! Configurable immutable runtime inputs; labels never determine protocol compatibility.
use crate::*;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BuildInputFile {
    pub path: String,
    pub content: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeRuntimeRelease {
    pub url: String,
    pub sha256: String,
    pub member: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionRecipe {
    pub provider: HarnessProvider,
    pub version: String,
    pub license: String,
    #[serde(default)]
    pub files: Vec<BuildInputFile>,
    #[serde(default)]
    pub image: Option<String>,
    #[serde(default)]
    pub native_releases: BTreeMap<String, NativeRuntimeRelease>,
}
pub fn build_input_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 1024
        && !path.contains('\\')
        && !path.contains('\0')
        && path
            .split('/')
            .all(|s| !s.is_empty() && s != "." && s != "..")
}
impl ExecutionRecipe {
    pub fn validate(&self) -> Result<()> {
        require(
            !self.version.is_empty()
                && self.version.len() <= 160
                && self
                    .version
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "._+-".contains(c)),
            "执行安装标签格式无效",
        )?;
        require(!self.license.is_empty(), "执行安装需要许可证/来源说明")?;
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
                    build_input_path(&file.path) && paths.insert(&file.path),
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
                    && build_input_path(&release.member),
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

/// Install inputs owned by a Harness; the version is Harness.runtime_version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HarnessInstallation {
    pub package: String,
    pub license: String,
    pub entrypoint: Vec<String>,
    #[serde(default)]
    pub environment: BTreeMap<String, String>,
    #[serde(default)]
    pub files: Vec<BuildInputFile>,
    #[serde(default)]
    pub native_releases: BTreeMap<String, NativeRuntimeRelease>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SandboxBuild {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub prebuilt_images: BTreeMap<String, String>,
    pub base_image: String,
    pub packages: Vec<String>,
    #[serde(default)]
    pub files: Vec<BuildInputFile>,
}
pub fn resolve_configuration_edge<'a>(
    state: &'a EngineeringState,
    edge: &ConfigurationEdge,
) -> Result<&'a ConfigurationRecord> {
    state.configuration(&edge.to)
}
pub fn compose_execution_recipe(
    harness: &ConfigurationRecord,
    sandbox: &ConfigurationRecord,
) -> Result<ExecutionRecipe> {
    let h: HarnessDefinition = harness.decode()?;
    let b: SandboxDefinition = sandbox.decode()?;
    let i = h
        .installation
        .ok_or_else(|| err("configuration_required", "Harness 缺少安装声明"))?;
    let build = b
        .build
        .ok_or_else(|| err("configuration_required", "Sandbox 缺少构建声明"))?;
    require(
        build
            .base_image
            .split_once("@sha256:")
            .is_some_and(|(_, d)| d.len() == 64 && d.bytes().all(|c| c.is_ascii_hexdigit())),
        "基础镜像需要不可变摘要",
    )?;
    require(
        i.package
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"@/._-".contains(&c))
            && !i.package.is_empty(),
        "无效 Harness 安装包",
    )?;
    require(
        build.packages.iter().all(|v| {
            !v.is_empty()
                && v.bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"+._-".contains(&c))
        }),
        "无效系统包",
    )?;
    require(!i.entrypoint.is_empty(), "Harness 缺少启动入口")?;
    let provider: HarnessProvider = serde_json::from_value(json!(h.provider))
        .map_err(|e| err("invalid_configuration", e.to_string()))?;
    let installation_identity = configuration_hash(
        &json!({"provider":h.provider,"version":h.runtime_version,"package":i.package,"files":i.files,"entrypoint":i.entrypoint,"environment":i.environment}),
    );
    if let Some(image) = build.prebuilt_images.get(&installation_identity) {
        let recipe = ExecutionRecipe {
            provider,
            version: h.runtime_version,
            license: i.license,
            files: vec![],
            image: Some(image.clone()),
            native_releases: i.native_releases,
        };
        recipe.validate()?;
        return Ok(recipe);
    }
    let mut files = build.files;
    files.extend(i.files);
    require(
        files.iter().all(|f| f.path != "Dockerfile"),
        "Dockerfile 由 Harness 与 Sandbox 组合生成",
    )?;
    let mut recipe = ExecutionRecipe {
        provider,
        version: h.runtime_version,
        license: i.license,
        files: vec![],
        image: None,
        native_releases: i.native_releases,
    };
    let mut dockerfile = format!(
        "FROM {}\nRUN apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install --yes --no-install-recommends {} && rm -rf /var/lib/apt/lists/*\n",
        build.base_image,
        build.packages.join(" ")
    );
    // The single version value also determines the package installed in the image.
    recipe.validate_version()?;
    dockerfile.push_str(&format!(
        "RUN npm install --global --omit=dev {}@{} && npm cache clean --force\n",
        i.package, recipe.version
    ));
    for file in &files {
        dockerfile.push_str(&format!(
            "COPY {} {}\n",
            serde_json::to_string(&file.path).unwrap(),
            serde_json::to_string(&format!("/opt/devdesk/{}", file.path)).unwrap()
        ));
    }
    for (key, value) in i.environment {
        require(
            key.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_'),
            "无效环境变量名",
        )?;
        dockerfile.push_str(&format!(
            "ENV {key}={}\n",
            serde_json::to_string(&value).unwrap()
        ));
    }
    dockerfile.push_str(&format!(
        "USER 1000:1000\nWORKDIR /work\nENTRYPOINT {}\n",
        serde_json::to_string(&i.entrypoint).unwrap()
    ));
    recipe.files.push(BuildInputFile {
        path: "Dockerfile".into(),
        content: dockerfile,
    });
    recipe.files.extend(files);
    recipe.validate()?;
    Ok(recipe)
}
impl ExecutionRecipe {
    fn validate_version(&self) -> Result<()> {
        require(
            !self.version.is_empty()
                && self.version.len() <= 160
                && self
                    .version
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"._+-".contains(&c)),
            "无效 Harness 版本",
        )
    }
}
pub fn harness_execution_recipe(
    state: &EngineeringState,
    harness: &ConfigurationRecord,
) -> Result<ExecutionRecipe> {
    let selection = permission_selection(state)?;
    compose_execution_recipe(harness, state.configuration(&selection.sandbox_ref)?)
}
