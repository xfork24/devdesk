//! Prepare immutable runtime sources from the selected configuration revision.
use super::*;
use std::{process::Command, time::Duration};

/// Default choices are read from the packaged catalog, never a Rust version table.
pub fn default_version(provider: HarnessProvider) -> String {
    packaged_recipe(provider).version
}
fn packaged_recipe(provider: HarnessProvider) -> ExecutionRecipe {
    factory_bundle::packaged_runtime(provider).expect("validated Harness/Sandbox installation")
}

pub fn default_codex_recipe_digest() -> String {
    packaged_recipe(HarnessProvider::Codex).recipe_digest()
}
pub fn default_dsh_recipe_digest() -> String {
    packaged_recipe(HarnessProvider::Dsh).recipe_digest()
}

impl GlobalStore {
    pub fn default_execution_recipe(
        &self,
        provider: HarnessProvider,
    ) -> Result<(ConfigurationRecord, ExecutionRecipe)> {
        let state = self.snapshot()?;
        let packaged = factory_bundle::packaged_harness(provider)?;
        let defaults = devdesk_core::current_template_defaults(&state).unwrap_or_default();
        let selected = defaults["harness_ref"]
            .as_str()
            .and_then(|id| state.configuration(id).ok())
            .filter(|r| r.data["provider"] == provider.as_str());
        let harness = selected.unwrap_or(state.configuration(&packaged.id)?);
        require(
            template_record_current(&state, harness) && !configuration_retired(&state, harness),
            "默认 Harness 已退出当前目录或被停用",
        )?;
        let selection = permission_selection(&state)?;
        let sandbox = state.configuration(&selection.sandbox_ref)?.clone();
        Ok((
            sandbox.clone(),
            compose_execution_recipe(harness, &sandbox)?,
        ))
    }
    pub fn prepare_docker_runtime(
        &self,
        provider: HarnessProvider,
        progress: impl Fn(&str),
    ) -> Result<RuntimeInstallation> {
        let (sandbox, recipe) = self.default_execution_recipe(provider)?;
        self.prepare_execution_recipe(&sandbox, &recipe, progress)
    }
    pub fn prepare_execution_recipe(
        &self,
        source: &ConfigurationRecord,
        resource: &ExecutionRecipe,
        progress: impl Fn(&str),
    ) -> Result<RuntimeInstallation> {
        resource.validate()?;
        let digest = resource.recipe_digest();
        self.preparation_intent(source, &digest, resource.provider, "preparing", None)?;
        let prepared: Result<RuntimeInstallation> = (|| {
            progress("正在检查 Docker 与所选 Harness/Sandbox");
            let daemon = process::capture(
                "docker",
                &["info".into(), "--format".into(), "{{.OSType}}".into()],
                Duration::from_secs(15),
            )?;
            require(
                daemon.code == 0 && String::from_utf8_lossy(&daemon.stdout).trim() == "linux",
                "需要已启动的 Linux Docker Engine",
            )?;
            let _image_lock = runtime_images::lock(resource.provider, &digest, || {
                self.preparation_checkpoint(&digest)
            })?;
            let registered = self.runtime_installations()?.into_iter().find(|i| {
                i.key.provider == resource.provider
                    && i.recipe_digest.as_deref() == Some(digest.as_str())
                    && matches!(&i.artifact, RuntimeArtifact::DockerImage { .. })
                    && self.runtime_integrity(&i.id).is_ok_and(|r| r.verified)
            });
            let mut tag = registered
                .as_ref()
                .and_then(|i| match &i.artifact {
                    RuntimeArtifact::DockerImage { image_id } => Some(image_id.clone()),
                    _ => None,
                })
                .unwrap_or_else(|| {
                    resource
                        .image
                        .clone()
                        .unwrap_or_else(|| runtime_images::recipe_tag(resource.provider, &digest))
                });
            let local = runtime_registry::docker_image_info(&tag)
                .ok()
                .filter(|info| {
                    resource.image.is_some() || info["labels"]["devdesk.recipe"] == digest
                });
            if registered.is_none() && local.is_none() {
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
                    let result: Result<()> = (|| {
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
                                "--label".into(),
                                format!("devdesk.recipe={digest}"),
                                context.to_string_lossy().into_owned(),
                            ],
                            Duration::from_secs(1200),
                        )?;
                        require(output.code == 0 && !output.truncated, "执行镜像构建失败")?;
                        tag = String::from_utf8(output.stdout)
                            .map_err(storage)?
                            .trim()
                            .to_owned();
                        Ok(())
                    })();
                    let _ = fs::remove_dir_all(&context);
                    result?;
                }
            }
            self.preparation_checkpoint(&digest)?;
            progress("正在核验实际镜像内容与平台");
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
            let retained =
                runtime_images::retain(resource.provider, &digest, &actual, registered.is_some())?;
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
            resource.provider,
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

    /// Program assets are shared by the host user; credentials remain dataset-local.
    pub fn prepare_codex_auth(&self, progress: impl Fn(&str)) -> Result<RuntimeInstallation> {
        self.prepare_codex_auth_cancellable(progress, &|| false)
    }
    pub fn prepare_codex_auth_cancellable(
        &self,
        progress: impl Fn(&str),
        cancelled: &dyn Fn() -> bool,
    ) -> Result<RuntimeInstallation> {
        use fs2::FileExt;
        let (_, resource) = self.default_execution_recipe(HarnessProvider::Codex)?;
        let platform = format!("{}/{}", std::env::consts::OS, std::env::consts::ARCH);
        let release = resource
            .native_releases
            .get(&platform)
            .ok_or_else(|| err("unsupported", "Harness 未提供本机登录组件"))?;
        let identity = configuration_hash(&(
            platform.as_str(),
            release.sha256.as_str(),
            release.member.as_str(),
        ));
        let cache = if let Some(path) = std::env::var_os("DEVDESK_RUNTIME_CACHE") {
            PathBuf::from(path)
        } else {
            let base = std::env::var_os("XDG_CACHE_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| {
                    PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".cache")
                });
            base.join("devdesk/installations")
        };
        require(cache.is_absolute(), "安装缓存必须使用绝对路径")?;
        reject_symlink(&cache)?;
        fs::create_dir_all(&cache).map_err(storage)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&cache, fs::Permissions::from_mode(0o700)).map_err(storage)?;
        }
        let directory = cache.join(format!("codex-{identity}"));
        reject_symlink(&directory)?;
        fs::create_dir_all(&directory).map_err(storage)?;
        let check_cancel = || require(!cancelled(), "登录准备已取消");
        let lock_path = directory.join("prepare.lock");
        reject_symlink(&lock_path)?;
        let lock = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(lock_path)
            .map_err(storage)?;
        progress("检查本地登录程序；等待同版本准备锁");
        loop {
            match lock.try_lock_exclusive() {
                Ok(()) => break,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    check_cancel()?;
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(e) => return Err(storage(e)),
            }
        }
        check_cancel()?;
        let binary = directory.join(&release.member);
        reject_symlink(&binary)?;
        let proof = directory.join("verified.json");
        reject_symlink(&proof)?;
        let verified = || -> Option<String> {
            let Ok(value) = fs::read(&proof)
                .and_then(|b| serde_json::from_slice::<Value>(&b).map_err(std::io::Error::other))
            else {
                return None;
            };
            let sha = runtime_registry::file_digest(&binary).ok()?;
            (value["archive_sha256"] == release.sha256
                && value["member"] == release.member
                && value["platform"] == platform
                && value["executable_sha256"] == sha)
                .then_some(sha)
        };
        progress("校验本地登录程序缓存");
        let mut executable_sha = verified();
        if executable_sha.is_none() {
            let staging = directory.join(format!("pending-{}", id()));
            fs::create_dir(&staging).map_err(storage)?;
            let result = (|| {
                let member = staging.join(&release.member);
                let existing = self.runtime_installations()?.into_iter().find(|r| {
                    r.key.provider == HarnessProvider::Codex
                        && r.key.version == resource.version
                        && r.key.os == std::env::consts::OS
                        && r.key.arch == std::env::consts::ARCH
                        && r.source == release.url
                        && matches!(r.artifact, RuntimeArtifact::Executable { .. })
                        && self.runtime_integrity(&r.id).is_ok_and(|r| r.verified)
                });
                if let Some(existing) = existing {
                    progress("复用已有已校验登录程序");
                    if let RuntimeArtifact::Executable { canonical_path, .. } = existing.artifact {
                        fs::copy(canonical_path, &member).map_err(storage)?;
                    }
                } else {
                    progress("下载固定版本登录程序");
                    let archive = staging.join("codex.tar.gz");
                    let bundled = std::env::current_exe().ok().and_then(|exe| {
                        exe.parent().map(|p| {
                            p.join("resources/installations")
                                .join(format!("{}.tar.gz", release.sha256))
                        })
                    });
                    if let Some(bundled) = bundled.filter(|p| p.is_file()) {
                        progress("复用随发行包准备的安装档案");
                        fs::copy(bundled, &archive).map_err(storage)?;
                    } else {
                        check_cancel()?;
                        let output = process::capture_cancellable(
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
                                "--connect-timeout".into(),
                                "15".into(),
                                "--max-time".into(),
                                "180".into(),
                                "--output".into(),
                                archive.to_string_lossy().into_owned(),
                                release.url.clone(),
                            ],
                            Duration::from_secs(185),
                            cancelled,
                        )?;
                        require(output.code == 0, "登录程序下载失败，请检查安装源网络")?;
                    }
                    progress("校验并解包登录程序");
                    require(
                        runtime_registry::file_digest(&archive)? == release.sha256,
                        "登录安装包摘要不匹配",
                    )?;
                    let mut command = Command::new("tar");
                    command
                        .arg("-xzf")
                        .arg(&archive)
                        .arg("-C")
                        .arg(&staging)
                        .arg(&release.member);
                    require(
                        process::capture_command(command, Duration::from_secs(30), None)?.code == 0,
                        "登录程序解包失败",
                    )?;
                }
                check_cancel()?;
                reject_symlink(&member)?;
                require(
                    fs::metadata(&member).map_err(storage)?.is_file(),
                    "安装件成员不是普通文件",
                )?;
                let sha = runtime_registry::file_digest(&member)?;
                let info = json!({"archive_sha256":release.sha256,"member":release.member,"platform":platform,"executable_sha256":sha});
                let pending = staging.join("verified.json");
                fs::write(&pending, serde_json::to_vec(&info).map_err(storage)?)
                    .map_err(storage)?;
                fs::rename(&member, &binary).map_err(storage)?;
                fs::rename(pending, &proof).map_err(storage)?;
                executable_sha = Some(sha);
                Ok(())
            })();
            let _ = fs::remove_dir_all(staging);
            result?;
        }
        let executable_sha =
            executable_sha.ok_or_else(|| err("provider_io", "登录程序缓存校验失败"))?;
        progress("本地登录程序已就绪");
        self.register_configured_runtime(
            RuntimeRegistration {
                key: RuntimeKey {
                    provider: HarnessProvider::Codex,
                    version: resource.version,
                    os: std::env::consts::OS.into(),
                    arch: std::env::consts::ARCH.into(),
                },
                artifact: RuntimeArtifact::Executable {
                    canonical_path: binary.to_string_lossy().into_owned(),
                    sha256: executable_sha,
                },
                source: release.url.clone(),
                license: resource.license,
            },
            Some(identity),
        )
    }
}
