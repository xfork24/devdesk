//! Prepare immutable runtime sources from the selected configuration revision.
use super::*;
use std::{process::Command, time::Duration};

/// Default choices are read from the packaged catalog, never a Rust version table.
pub fn default_version(provider: HarnessProvider) -> String {
    packaged_recipe(provider).version
}
fn packaged_recipe(provider: HarnessProvider) -> RuntimeResourceDefinition {
    serde_json::from_value(
        factory_bundle::packaged_runtime(provider)
            .expect("packaged runtime selection")
            .data,
    )
    .expect("validated runtime source")
}
pub fn default_codex_recipe_digest() -> String {
    packaged_recipe(HarnessProvider::Codex).recipe_digest()
}
pub fn default_dsh_recipe_digest() -> String {
    packaged_recipe(HarnessProvider::Dsh).recipe_digest()
}

impl GlobalStore {
    pub fn default_runtime_resource(
        &self,
        provider: HarnessProvider,
    ) -> Result<ConfigurationRecord> {
        let state = self.snapshot()?;
        if let Some(reference) = current_template_defaults(&state).and_then(|v| {
            v["runtime_resources"][provider.as_str()]
                .as_str()
                .map(str::to_owned)
        }) {
            return Ok(state.configuration(&reference)?.clone());
        }
        factory_bundle::packaged_runtime(provider)?.into_record()
    }
    pub fn prepare_docker_runtime(
        &self,
        provider: HarnessProvider,
        progress: impl Fn(&str),
    ) -> Result<RuntimeInstallation> {
        self.prepare_runtime_resource(&self.default_runtime_resource(provider)?, progress)
    }
    pub fn prepare_runtime_resource(
        &self,
        source: &ConfigurationRecord,
        progress: impl Fn(&str),
    ) -> Result<RuntimeInstallation> {
        require(
            source.r#type == "runtime_resource" && source.immutable,
            "需要不可变运行资源源修订",
        )?;
        let resource: RuntimeResourceDefinition = source.decode()?;
        resource.validate()?;
        let digest = resource.recipe_digest();
        self.preparation_intent(source, &digest, "preparing", None)?;
        let prepared: Result<RuntimeInstallation> = (|| {
            progress("正在检查 Docker 与所选运行资源");
            let daemon = process::capture(
                "docker",
                &["info".into(), "--format".into(), "{{.OSType}}".into()],
                Duration::from_secs(15),
            )?;
            require(
                daemon.code == 0 && String::from_utf8_lossy(&daemon.stdout).trim() == "linux",
                "需要已启动的 Linux Docker Engine",
            )?;
            let registered = self.runtime_installations()?.into_iter().find(|i| {
                i.key.provider == resource.provider
                    && i.recipe_digest.as_deref() == Some(digest.as_str())
                    && matches!(&i.artifact, RuntimeArtifact::DockerImage { .. })
                    && self.runtime_integrity(&i.id).is_ok_and(|r| r.verified)
            });
            let tag = registered
                .as_ref()
                .and_then(|i| match &i.artifact {
                    RuntimeArtifact::DockerImage { image_id } => Some(image_id.clone()),
                    _ => None,
                })
                .unwrap_or_else(|| {
                    resource.image.clone().unwrap_or_else(|| {
                        format!(
                            "devdesk/managed-{}:{}",
                            resource.provider.as_str(),
                            &digest[..24]
                        )
                    })
                });
            if registered.is_none() {
                if resource.image.is_some() {
                    progress("正在获取不可变镜像");
                    let output = process::capture(
                        "docker",
                        &["pull".into(), tag.clone()],
                        Duration::from_secs(1200),
                    )?;
                    require(output.code == 0, "获取运行镜像失败")?;
                } else {
                    progress("正在构建所选不可变配方");
                    let context = self.runtime_directory()?.join(format!("build-{}", id()));
                    fs::create_dir(&context).map_err(storage)?;
                    let result = (|| {
                        for file in &resource.files {
                            let path = context.join(&file.path);
                            fs::create_dir_all(path.parent().unwrap()).map_err(storage)?;
                            fs::write(path, &file.content).map_err(storage)?;
                        }
                        let output = process::capture(
                            "docker",
                            &[
                                "build".into(),
                                "--quiet".into(),
                                "--tag".into(),
                                tag.clone(),
                                context.to_string_lossy().into_owned(),
                            ],
                            Duration::from_secs(1200),
                        )?;
                        require(output.code == 0, "运行资源构建失败")
                    })();
                    let _ = fs::remove_dir_all(&context);
                    result?;
                }
            }
            self.preparation_checkpoint(&digest)?;
            progress("正在核验实际镜像内容与平台");
            // Rebuilding the same recipe in another dataset may replace its build tag.
            // Retain this exact artifact before registering/freezing it: containerd can
            // otherwise lose an untagged OCI index even while an older worker exists.
            let actual = runtime_registry::docker_image_info(&tag)?;
            let image = actual["id"]
                .as_str()
                .ok_or_else(|| err("provider_io", "镜像未提供内容身份"))?;
            require(
                image.len() == 71
                    && image.starts_with("sha256:")
                    && image[7..].bytes().all(|c| c.is_ascii_hexdigit()),
                "Docker 镜像内容身份无效",
            )?;
            let retained = format!(
                "devdesk/prepared-{}:{}",
                resource.provider.as_str(),
                &image[7..]
            );
            let output = process::capture(
                "docker",
                &["image".into(), "tag".into(), image.into(), retained.clone()],
                Duration::from_secs(30),
            )?;
            require(
                output.code == 0 && runtime_registry::docker_image_info(&retained)?["id"] == *image,
                "无法保留所选运行产物，请重新核对准备结果",
            )?;
            let installation = self.register_docker_runtime(
                resource.provider,
                &retained,
                &resource.version,
                &digest,
                &resource.license,
            )?;
            progress("正在验证实际运行协议");
            self.verify_runtime_protocol(&installation)?;
            self.preparation_checkpoint(&digest)?;
            Ok(installation)
        })();
        let result = match &prepared {
            Ok(i) => json!({"installation_id":i.id}),
            Err(e) => json!({"code":e.code,"message":e.message}),
        };
        self.preparation_intent(
            source,
            &digest,
            if prepared.is_ok() {
                "ready"
            } else if prepared
                .as_ref()
                .is_err_and(|e| e.code == "needs_reconciliation")
            {
                "preparing"
            } else {
                "failed"
            },
            Some(&result),
        )?;
        prepared
    }

    fn runtime_directory(&self) -> Result<PathBuf> {
        let path = Path::new(&self.root()).join("runtimes");
        reject_symlink(&path)?;
        fs::create_dir_all(&path).map_err(storage)?;
        Ok(path)
    }

    /// Authentication needs a host binary, but never the user's desktop CODEX_HOME.
    pub fn prepare_codex_auth(&self, progress: impl Fn(&str)) -> Result<RuntimeInstallation> {
        let source = self.default_runtime_resource(HarnessProvider::Codex)?;
        let resource: RuntimeResourceDefinition = source.decode()?;
        resource.validate()?;
        let version = &resource.version;
        let native_identity = source.content_hash.clone();
        let release = resource
            .native_releases
            .get(&format!(
                "{}/{}",
                std::env::consts::OS,
                std::env::consts::ARCH
            ))
            .ok_or_else(|| err("unsupported", "所选资源没有当前平台的登录组件"))?;
        let target = &release.member;
        let checksum = &release.sha256;
        progress("正在准备 Codex 登录组件");
        if let Some(runtime) = self.runtime_installations()?.into_iter().find(|r| {
            r.key.provider == HarnessProvider::Codex
                && r.recipe_digest.as_deref() == Some(native_identity.as_str())
                && r.key.os == std::env::consts::OS
                && r.key.arch == std::env::consts::ARCH
                && matches!(r.artifact, RuntimeArtifact::Executable { .. })
        }) {
            require(
                self.runtime_integrity(&runtime.id)?.verified,
                "已保存的 Codex 登录组件发生漂移，请检查运行时文件",
            )?;
            return Ok(runtime);
        }
        let directory = self
            .runtime_directory()?
            .join(format!("codex-{}-{target}", &native_identity[..24]));
        reject_symlink(&directory)?;
        fs::create_dir_all(&directory).map_err(storage)?;
        let binary = directory.join(target.clone());
        reject_symlink(&binary)?;
        {
            progress("正在下载并校验 Codex 官方登录组件");
            let staging = self.runtime_directory()?.join(format!("download-{}", id()));
            fs::create_dir(&staging).map_err(storage)?;
            let result = (|| {
                let archive = staging.join("codex.tar.gz");
                let url = release.url.clone();
                let downloaded = process::capture(
                    "curl",
                    &[
                        "--fail".into(),
                        "--silent".into(),
                        "--show-error".into(),
                        "--location".into(),
                        "--proto".into(),
                        "=https".into(),
                        "--proto-redir".into(),
                        "=https".into(),
                        "--max-time".into(),
                        "600".into(),
                        "--output".into(),
                        archive.to_string_lossy().into_owned(),
                        url,
                    ],
                    Duration::from_secs(610),
                )
                .map_err(|_| {
                    err(
                        "runtime_preparation_failed",
                        "登录组件下载超时或无法启动，请检查网络和 curl 后重试",
                    )
                })?;
                require(
                    downloaded.code == 0,
                    "Codex 登录组件下载失败，请检查 GitHub 网络访问后重试",
                )?;
                require(
                    runtime_registry::file_digest(&archive)? == *checksum,
                    "Codex 官方安装包校验失败",
                )?;
                let mut command = Command::new("tar");
                command
                    .arg("-xzf")
                    .arg(&archive)
                    .arg("-C")
                    .arg(&staging)
                    .arg(target.clone());
                let unpacked = process::capture_command(command, Duration::from_secs(30), None)?;
                require(unpacked.code == 0, "Codex 登录组件解包失败")?;
                let member = staging.join(target);
                reject_symlink(&member)?;
                require(
                    fs::metadata(&member).map_err(storage)?.is_file(),
                    "下载成员必须是普通文件",
                )?;
                fs::create_dir_all(binary.parent().unwrap()).map_err(storage)?;
                fs::rename(member, &binary).map_err(storage)
            })();
            let _ = fs::remove_dir_all(staging);
            result?;
        }
        self.register_configured_runtime(
            RuntimeRegistration {
                key: RuntimeKey {
                    provider: HarnessProvider::Codex,
                    version: version.clone(),
                    os: std::env::consts::OS.into(),
                    arch: std::env::consts::ARCH.into(),
                },
                artifact: RuntimeArtifact::Executable {
                    canonical_path: binary.to_string_lossy().into_owned(),
                    sha256: runtime_registry::file_digest(&binary)?,
                },
                source: release.url.clone(),
                license: resource.license.clone(),
            },
            Some(native_identity),
        )
    }
}
