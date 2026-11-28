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
impl NativeRuntimeRelease {
    fn validate(&self) -> Result<()> {
        let url =
            url::Url::parse(&self.url).map_err(|_| err("invalid_source", "运行下载 URL 无效"))?;
        require(
            url.scheme() == "https"
                && url.username().is_empty()
                && url.password().is_none()
                && url.query().is_none()
                && url.fragment().is_none(),
            "运行下载使用无内嵌凭据的 HTTPS 地址",
        )?;
        require(
            self.sha256.len() == 64
                && self.sha256.bytes().all(|b| b.is_ascii_hexdigit())
                && build_input_path(&self.member),
            "运行下载需要完整摘要和安全成员路径",
        )
    }
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
            release.validate()?;
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub npm_options: Option<NpmInstallationOptions>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub container_releases: BTreeMap<String, NativeRuntimeRelease>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zcode_cli_source: Option<ZcodeCliSource>,
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
pub struct NpmInstallationOptions {
    pub version: String,
    #[serde(default)]
    pub include_optional: bool,
    #[serde(default)]
    pub allow_scripts: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ZcodeCliSource {
    pub commit: String,
    pub sha256: String,
}

fn npm_package_name(package: &str) -> bool {
    fn component(value: &str) -> bool {
        value
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric)
            && value
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
    }
    package.len() <= 214
        && match package.strip_prefix('@') {
            Some(scoped) => scoped
                .split_once('/')
                .is_some_and(|(scope, name)| component(scope) && component(name)),
            None => component(package),
        }
}

impl HarnessInstallation {
    fn install_instructions(&self, version: &str) -> Result<String> {
        if let Some(source) = &self.zcode_cli_source {
            let hex = |value: &str, len| {
                value.len() == len
                    && value
                        .bytes()
                        .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
            };
            require(
                self.package.is_none()
                    && self.npm_options.is_none()
                    && self.container_releases.is_empty()
                    && hex(&source.commit, 40)
                    && hex(&source.sha256, 64),
                "ZCode CLI 需要独占的固定源码提交及完整摘要",
            )?;
            return Ok(String::new());
        }
        if let Some(package) = &self.package {
            // Keep the original source validation and bytes for existing npm recipes.
            require(
                !package.is_empty()
                    && package
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || b"@/._-".contains(&c))
                    && !package.starts_with('-')
                    && self.container_releases.is_empty(),
                "无效 Harness 安装包",
            )?;
            let Some(options) = &self.npm_options else {
                return Ok(format!(
                    "RUN npm install --global --omit=dev {package}@{version} && npm cache clean --force\n"
                ));
            };
            require(
                options.version == "11.16.0"
                    && npm_package_name(package)
                    && options.allow_scripts.len() <= 32
                    && options
                        .allow_scripts
                        .iter()
                        .all(|name| npm_package_name(name)),
                "npm 脚本策略需要 npm 11.16.0 和有限的包名白名单",
            )?;
            let mut args = vec![
                "npm".to_string(),
                "install".into(),
                "--global".into(),
                "--omit=dev".into(),
                "--strict-allow-scripts".into(),
            ];
            if options.include_optional {
                args.push("--include=optional".into());
            }
            if !options.allow_scripts.is_empty() {
                args.push(format!(
                    "--allow-scripts={}",
                    options.allow_scripts.join(",")
                ));
            }
            args.push(format!("{package}@{version}"));
            return Ok(format!(
                "RUN npm install --global --ignore-scripts npm@{}\nRUN {}\nRUN npm cache clean --force\n",
                options.version,
                json!(args)
            ));
        }

        require(
            self.npm_options.is_none()
                && self.container_releases.len() == 2
                && ["amd64", "arm64"]
                    .iter()
                    .all(|arch| self.container_releases.contains_key(*arch)),
            "容器二进制安装需要 amd64 和 arm64 的固定发行资产",
        )?;
        let executable = &self.entrypoint[0];
        self.container_releases
            .values()
            .try_for_each(NativeRuntimeRelease::validate)?;
        require(
            npm_package_name(executable) && !executable.contains('/'),
            "容器二进制安装名称无效",
        )?;
        let archives = json!(self.container_releases).to_string();
        Ok(format!(
            "ARG TARGETARCH\nRUN {}\n",
            json!([
                "node",
                "--input-type=module",
                "-e",
                INSTALL_ARCHIVE,
                archives,
                executable
            ])
        ))
    }
}

const INSTALL_ARCHIVE: &str = r#"
import { createHash } from 'node:crypto';
import { closeSync, createWriteStream, mkdtempSync, openSync, rmSync, statSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import { Readable, Transform } from 'node:stream';
import { pipeline } from 'node:stream/promises';
const release = JSON.parse(process.argv[1])[process.env.TARGETARCH];
if (!release) throw new Error('Unsupported target architecture');
const scratch = mkdtempSync('/tmp/devdesk-release-');
try {
  const archive = `${scratch}/release.tar`;
  const response = await fetch(release.url, { signal: AbortSignal.timeout(300_000) });
  if (!response.ok || !response.body || !response.url.startsWith('https://')) {
    throw new Error('Runtime archive download failed');
  }
  const hash = createHash('sha256');
  let bytes = 0;
  const checksum = new Transform({ transform(chunk, _, done) {
    bytes += chunk.length;
    if (bytes > 512 * 1024 * 1024) return done(new Error('Runtime archive exceeds size limit'));
    hash.update(chunk);
    done(null, chunk);
  } });
  await pipeline(Readable.fromWeb(response.body), checksum, createWriteStream(archive, { mode: 0o600 }));
  if (hash.digest('hex') !== release.sha256.toLowerCase()) throw new Error('Runtime archive digest mismatch');
  const destination = `/usr/local/bin/${process.argv[2]}`;
  const output = openSync(destination, 'wx', 0o755);
  try {
    execFileSync('tar', ['-xOf', archive, '--', release.member], { stdio: ['ignore', output, 'inherit'] });
  } finally {
    closeSync(output);
  }
  if (statSync(destination).size === 0) throw new Error('Runtime archive member is empty');
} finally {
  rmSync(scratch, { recursive: true, force: true });
}
"#;
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
    let install_instructions = i.install_instructions(&h.runtime_version)?;
    require(
        i.zcode_cli_source.is_none()
            || (provider == HarnessProvider::Zcode && i.entrypoint == ["zcode", "app-server"]),
        "固定 ZCode CLI 源码仅用于 ZCode app-server",
    )?;
    let mut identity = json!({"provider":h.provider,"version":h.runtime_version,"package":i.package,"files":i.files,"entrypoint":i.entrypoint,"environment":i.environment});
    if let Some(options) = &i.npm_options {
        identity["npm_options"] = json!(options);
    }
    if !i.container_releases.is_empty() {
        identity["container_releases"] = json!(i.container_releases);
    }
    if let Some(source) = &i.zcode_cli_source {
        identity["zcode_cli_source"] = json!(source);
    }
    let installation_identity = configuration_hash(&identity);
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
    const ZCODE_INSTALLER: &str = "zcode/build-cli.sh";
    if i.zcode_cli_source.is_some() {
        files.push(BuildInputFile {
            path: ZCODE_INSTALLER.into(),
            content: include_str!("../../../scripts/release/build-zcode-cli.sh").into(),
        });
    }
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
        "FROM {}\nRUN apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install --yes --no-install-recommends {}{} && rm -rf /var/lib/apt/lists/*\n",
        build.base_image,
        build.packages.join(" "),
        if i.zcode_cli_source.is_some() {
            " curl"
        } else {
            ""
        },
    );
    // The single version value also determines the package installed in the image.
    recipe.validate_version()?;
    dockerfile.push_str(&install_instructions);
    for file in &files {
        dockerfile.push_str(&format!(
            "COPY {} {}\n",
            serde_json::to_string(&file.path).unwrap(),
            serde_json::to_string(&format!("/opt/devdesk/{}", file.path)).unwrap()
        ));
    }
    if let Some(source) = i.zcode_cli_source {
        dockerfile.push_str(&format!(
            "RUN {}\nRUN ln -s /opt/devdesk/zcode-cli/zcode /usr/local/bin/zcode\n",
            json!([
                "/bin/sh",
                format!("/opt/devdesk/{ZCODE_INSTALLER}"),
                source.commit,
                source.sha256,
                "/opt/devdesk/zcode-cli"
            ])
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
