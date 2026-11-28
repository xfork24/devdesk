//! Artifact registration has no process, network, installation or deletion side effects.
use super::*;
use std::io::Read;

pub fn configuration_digest(value: &Value) -> Result<String> {
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(value).map_err(storage)?)
    ))
}

pub(crate) fn file_digest(path: &Path) -> Result<String> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    let mut file = File::open(path).map_err(storage)?;
    require(
        file.metadata().map_err(storage)?.is_file(),
        "运行时路径不是文件",
    )?;
    let mut hash = Sha256::new();
    let mut chunk = [0; 65536];
    loop {
        require(
            std::time::Instant::now() < deadline,
            "运行时文件摘要校验超时，请检查文件位置",
        )?;
        let n = file.read(&mut chunk).map_err(storage)?;
        if n == 0 {
            break;
        }
        hash.update(&chunk[..n]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

fn checked_artifact(artifact: RuntimeArtifact, key: &RuntimeKey) -> Result<RuntimeArtifact> {
    match artifact {
        RuntimeArtifact::Executable {
            canonical_path,
            sha256,
        } => {
            require(
                Path::new(&canonical_path).is_absolute(),
                "运行时路径必须是绝对路径",
            )?;
            require(
                sha256.len() == 64
                    && sha256
                        .bytes()
                        .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()),
                "需要明确的小写 SHA256",
            )?;
            let path = fs::canonicalize(canonical_path).map_err(storage)?;
            let actual = file_digest(&path)?;
            require(actual == sha256, "运行时内容摘要不匹配")?;
            // Scripts may rely on an untracked module tree; register their complete OCI image instead.
            let mut header = [0; 64];
            File::open(&path)
                .map_err(storage)?
                .read_exact(&mut header)
                .map_err(storage)?;
            require(
                !header.starts_with(b"#!"),
                "脚本入口不代表完整安装，请登记完整运行时镜像",
            )?;
            let platform = if header.starts_with(b"\x7fELF") && header[4] == 2 && header[5] == 1 {
                Some((
                    "linux",
                    match u16::from_le_bytes([header[18], header[19]]) {
                        183 => "aarch64",
                        62 => "x86_64",
                        _ => "unknown",
                    },
                ))
            } else if header.starts_with(&[0xcf, 0xfa, 0xed, 0xfe]) {
                Some((
                    "macos",
                    match u32::from_le_bytes(header[4..8].try_into().unwrap()) {
                        0x0100000c => "aarch64",
                        0x01000007 => "x86_64",
                        _ => "unknown",
                    },
                ))
            } else {
                None
            };
            require(
                platform == Some((key.os.as_str(), key.arch.as_str())),
                "可执行文件的平台不匹配或格式尚不支持；可改用完整运行时镜像",
            )?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                require(
                    fs::metadata(&path).map_err(storage)?.permissions().mode() & 0o111 != 0,
                    "运行时文件不可执行",
                )?;
            }
            Ok(RuntimeArtifact::Executable {
                canonical_path: path.to_string_lossy().into_owned(),
                sha256: actual,
            })
        }
        RuntimeArtifact::OciImage { reference } => {
            let (name, digest) = reference
                .split_once("@sha256:")
                .ok_or_else(|| err("invalid_params", "镜像必须以 SHA256 digest 固定"))?;
            require(
                !name.is_empty()
                    && !name.starts_with('-')
                    && name
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || b"/._:-".contains(&c)),
                "无效镜像名称",
            )?;
            require(
                digest.len() == 64
                    && digest
                        .bytes()
                        .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)),
                "无效镜像摘要",
            )?;
            Ok(RuntimeArtifact::OciImage { reference })
        }
        RuntimeArtifact::DockerImage { image_id } => {
            require(
                image_id.starts_with("sha256:")
                    && image_id.len() == 71
                    && image_id[7..]
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
                "本机 Docker 镜像必须使用明确 digest",
            )?;
            Ok(RuntimeArtifact::DockerImage { image_id })
        }
    }
}

fn same_content(prior: &RuntimeInstallation, registration: &RuntimeRegistration) -> Result<()> {
    require(
        prior.key == registration.key,
        "安装归属与版本/平台身份不一致",
    )?;
    let same = match (&prior.artifact, &registration.artifact) {
        (
            RuntimeArtifact::Executable { sha256: a, .. },
            RuntimeArtifact::Executable { sha256: b, .. },
        ) => a == b,
        (a, b) => a == b,
    };
    require(same, "同版本/平台安装内容冲突，请提供新的 build identity")
}
fn verified_registration(
    mut registration: RuntimeRegistration,
    recipe_digest: Option<&str>,
) -> Result<RuntimeRegistration> {
    if let Some(digest) = recipe_digest {
        require(
            digest.len() == 64 && digest.bytes().all(|b| b.is_ascii_hexdigit()),
            "配方摘要格式无效",
        )?;
    }
    registration.key.validate()?;
    registration.artifact = checked_artifact(registration.artifact, &registration.key)?;
    Ok(registration)
}

fn read_installation(row: &rusqlite::Row<'_>) -> rusqlite::Result<String> {
    row.get(0)
}
const SELECT: &str = "SELECT json_object('id',id,'key',json_object('provider',provider,'version',version,'os',os,'arch',arch),'artifact',json(artifact),'source',source,'license',license,'recipe_digest',recipe_digest) FROM runtime_installations";

impl GlobalStore {
    pub fn discover_docker_runtime(
        &self,
        provider: HarnessProvider,
    ) -> Result<RuntimeInstallation> {
        let recipe = runtime_preparation::packaged_recipe(provider);
        let tag = format!("devdesk/{}:{}", provider.as_str(), recipe.version);
        self.register_docker_runtime(
            provider,
            &tag,
            &recipe.version,
            &recipe.recipe_digest(),
            &recipe.license,
        )
    }
    pub(crate) fn register_docker_runtime(
        &self,
        provider: HarnessProvider,
        tag: &str,
        version: &str,
        recipe_digest: &str,
        license: &str,
    ) -> Result<RuntimeInstallation> {
        let value = docker_image_info(tag)?;
        let image = value["id"]
            .as_str()
            .ok_or_else(|| err("provider_io", "镜像未提供内容身份"))?;
        require(
            image.len() == 71
                && image.starts_with("sha256:")
                && image[7..]
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "Docker 镜像内容身份无效",
        )?;
        let arch = normalized_arch(value["arch"].as_str().unwrap_or(""));
        self.register_configured_runtime(
            RuntimeRegistration {
                key: RuntimeKey {
                    provider,
                    // The recipe identity is the content anchor; permission acceptance and
                    // harness materialization compare it, never the base version string.
                    version: version.into(),
                    os: value["os"].as_str().unwrap_or("").into(),
                    arch: arch.into(),
                },
                artifact: RuntimeArtifact::DockerImage {
                    image_id: image.into(),
                },
                source: format!("local Docker build: {tag}"),
                license: license.into(),
            },
            Some(recipe_digest.into()),
        )
    }
    pub fn discover_codex(&self) -> Result<RuntimeInstallation> {
        let path = std::env::var_os("PATH")
            .and_then(|paths| {
                std::env::split_paths(&paths)
                    .map(|p| p.join(if cfg!(windows) { "codex.exe" } else { "codex" }))
                    .find(|p| p.is_file())
            })
            .ok_or_else(|| err("provider_unavailable", "PATH 中未找到 Codex 原生安装"))?;
        let path = fs::canonicalize(path).map_err(storage)?;
        let digest = file_digest(&path)?;
        let key = RuntimeKey {
            provider: HarnessProvider::Codex,
            version: "probe".into(),
            os: std::env::consts::OS.into(),
            arch: std::env::consts::ARCH.into(),
        };
        let artifact = checked_artifact(
            RuntimeArtifact::Executable {
                canonical_path: path.to_string_lossy().into_owned(),
                sha256: digest,
            },
            &key,
        )?;
        let output = crate::process::capture(
            path.to_str()
                .ok_or_else(|| err("blocked", "Codex 路径需要 UTF-8"))?,
            &["--version".into()],
            std::time::Duration::from_secs(5),
        )?;
        require(output.code == 0 && !output.truncated, "Codex 版本查询失败")?;
        let version = String::from_utf8(output.stdout).map_err(storage)?;
        let version = version
            .trim()
            .strip_prefix("codex-cli ")
            .ok_or_else(|| err("blocked", "无法识别 Codex 版本"))?;
        self.register_runtime(RuntimeRegistration {
            key: RuntimeKey {
                version: version.into(),
                ..key
            },
            artifact,
            source: "https://github.com/openai/codex".into(),
            license: "Apache-2.0".into(),
        })
    }
    pub fn runtime_installations(&self) -> Result<Vec<RuntimeInstallation>> {
        let conn = self.connection.read().map_err(storage)?;
        let mut query = conn
            .prepare(&format!("{SELECT} ORDER BY provider,version,os,arch"))
            .map_err(storage)?;
        query
            .query_map([], read_installation)
            .map_err(storage)?
            .map(|row| serde_json::from_str(&row.map_err(storage)?).map_err(storage))
            .collect()
    }

    pub fn register_runtime(
        &self,
        registration: RuntimeRegistration,
    ) -> Result<RuntimeInstallation> {
        self.register_configured_runtime(registration, None)
    }
    pub(crate) fn register_configured_runtime(
        &self,
        registration: RuntimeRegistration,
        recipe_digest: Option<String>,
    ) -> Result<RuntimeInstallation> {
        let registration = verified_registration(registration, recipe_digest.as_deref())?;
        let key = devdesk_core::runtime_registry::installation_id(
            &registration.key,
            recipe_digest.as_deref(),
        );
        if let Some(prior) = devdesk_core::runtime_registry::read_installation(self, &key)? {
            same_content(&prior, &registration)?;
            return Ok(prior);
        }
        match devdesk_core::runtime_registry::recover_verified_missing(
            self,
            registration.clone(),
            recipe_digest.clone(),
        ) {
            Ok(Some(recovered)) => return Ok(recovered),
            Ok(None) => {}
            Err(error) if error.code == "revision_conflict" => {
                let prior =
                    devdesk_core::runtime_registry::read_installation(self, &key)?.ok_or(error)?;
                same_content(&prior, &registration)?;
                return Ok(prior);
            }
            Err(error) => return Err(error),
        }
        let command = Command {
            command_id: id(),
            target: Some(key.clone()),
            expected_revision: Some(0),
            payload: json!(registration),
        };
        let context = InvocationContext::human(None);
        match devdesk_core::runtime_registry::register_verified(
            self,
            &command,
            registration.clone(),
            recipe_digest,
            &context,
        ) {
            Ok(receipt) => serde_json::from_value(receipt.result).map_err(storage),
            Err(error) if error.code == "revision_conflict" => {
                // Internal materialization deduplicates the same verified content. No old
                // user version is refreshed, and no external effect is repeated here.
                let prior =
                    devdesk_core::runtime_registry::read_installation(self, &key)?.ok_or(error)?;
                same_content(&prior, &registration)?;
                Ok(prior)
            }
            Err(error) => Err(error),
        }
    }

    pub fn register_runtime_command(
        &self,
        command: &Command,
        context: &InvocationContext,
    ) -> Result<devdesk_core::concurrency::CommitReceipt> {
        let fingerprint =
            devdesk_core::runtime_registry::registration_fingerprint(command, context);
        if let Some(receipt) = self.replay_commit(&command.command_id, &fingerprint)? {
            return Ok(receipt);
        }
        devdesk_core::runtime_registry::registration_preconditions(command)?;
        require(
            command.payload.as_object().is_some_and(|fields| {
                fields
                    .keys()
                    .all(|key| ["key", "artifact", "source", "license"].contains(&key.as_str()))
            }),
            "安装登记正文包含未知字段",
        )?;
        let registration: RuntimeRegistration = serde_json::from_value(command.payload.clone())
            .map_err(|error| err("invalid_params", error.to_string()))?;
        let registration = verified_registration(registration, None)?;
        devdesk_core::runtime_registry::register_verified(
            self,
            command,
            registration,
            None,
            context,
        )
    }

    pub fn runtime_integrity(&self, id: &str) -> Result<RuntimeIntegrity> {
        let installation = self
            .runtime_installations()?
            .into_iter()
            .find(|r| r.id == id)
            .ok_or_else(|| err("not_found", "运行时安装不存在"))?;
        let (verified, reason) = match &installation.artifact {
            RuntimeArtifact::Executable {
                canonical_path,
                sha256,
            } => match file_digest(Path::new(canonical_path)) {
                Ok(actual) if &actual == sha256 => {
                    (true, "内容摘要匹配；尚未验证协议、认证与执行能力")
                }
                Ok(_) => (false, "运行时文件发生漂移"),
                Err(_) => (false, "运行时文件不可读"),
            },
            RuntimeArtifact::OciImage { .. } => (
                false,
                "仅登记不可变镜像引用；需要 Docker 服务核验镜像与平台",
            ),
            RuntimeArtifact::DockerImage { image_id } => match docker_image_info(image_id) {
                Ok(info)
                    if info["id"] == *image_id
                        && info["os"] == installation.key.os
                        && normalized_arch(info["arch"].as_str().unwrap_or(""))
                            == installation.key.arch =>
                {
                    (
                        true,
                        "Docker 镜像内容与平台匹配；不代表认证或真实执行已验收",
                    )
                }
                Ok(_) => (false, "Docker 镜像平台或内容身份不匹配"),
                Err(_) => (false, "无法核对 Docker 镜像；请检查 daemon 与已安装镜像"),
            },
        };
        Ok(RuntimeIntegrity {
            installation_id: installation.id,
            verified,
            reason: reason.into(),
            inference_verified: false,
        })
    }
}

fn normalized_arch(arch: &str) -> &str {
    match arch {
        "arm64" => "aarch64",
        "amd64" => "x86_64",
        other => other,
    }
}
pub(crate) fn docker_image_info(reference: &str) -> Result<Value> {
    let output = crate::process::capture(
        "docker",
        &[
            "image".into(),
            "inspect".into(),
            "--format".into(),
            "{\"id\":{{json .Id}},\"os\":{{json .Os}},\"arch\":{{json .Architecture}},\"labels\":{{json .Config.Labels}},\"tags\":{{json .RepoTags}}}".into(),
            reference.into(),
        ],
        std::time::Duration::from_secs(15),
    )?;
    require(
        output.code == 0 && !output.truncated,
        "无法核对本机 Docker 运行时镜像",
    )?;
    serde_json::from_slice(&output.stdout).map_err(storage)
}
