//! Managed, version-pinned runtime preparation. Build inputs are embedded so released hosts
//! work without the source checkout, npm, or a development toolchain.
use super::*;
use std::{process::Command, time::Duration};

pub const CODEX_VERSION: &str = DEFAULT_CODEX_VERSION;
const CODEX_FILES: &[(&str, &str)] = &[(
    "Dockerfile",
    include_str!("../../../containers/codex/Dockerfile"),
)];
const DSH_FILES: &[(&str, &str)] = &[
    (
        "Dockerfile",
        include_str!("../../../containers/dsh/Dockerfile"),
    ),
    (
        "permissions/backends/dsh/package.json",
        include_str!("../../../permissions/backends/dsh/package.json"),
    ),
    (
        "permissions/backends/dsh/tool-policy.mjs",
        include_str!("../../../permissions/backends/dsh/tool-policy.mjs"),
    ),
    (
        "permissions/backends/dsh/bwrap-runner.sh",
        include_str!("../../../permissions/backends/dsh/bwrap-runner.sh"),
    ),
];

fn recipe(
    provider: HarnessProvider,
) -> (
    &'static str,
    &'static str,
    &'static [(&'static str, &'static str)],
) {
    match provider {
        HarnessProvider::Codex => (CODEX_VERSION, "Apache-2.0", CODEX_FILES),
        HarnessProvider::Dsh => (DEFAULT_DSH_VERSION, "MIT", DSH_FILES),
    }
}

impl GlobalStore {
    pub fn prepare_docker_runtime(
        &self,
        provider: HarnessProvider,
        progress: impl Fn(&str),
    ) -> Result<RuntimeInstallation> {
        let (version, license, files) = recipe(provider);
        let digest = configuration_digest(&json!(files))?;
        let tag = format!(
            "devdesk/managed-{}:{version}-{}",
            provider.as_str(),
            &digest[..16]
        );
        progress("正在检查 Docker 与共享运行时");
        let daemon = process::capture(
            "docker",
            &["info".into(), "--format".into(), "{{.OSType}}".into()],
            Duration::from_secs(15),
        )
        .map_err(|_| {
            err(
                "environment_required",
                "无法连接 Docker；请安装并启动 Docker Desktop 或 Docker Engine，然后重试",
            )
        })?;
        require(
            daemon.code == 0 && String::from_utf8_lossy(&daemon.stdout).trim() == "linux",
            "需要已启动的 Linux Docker Engine；请检查 Docker Desktop / Engine",
        )?;
        if runtime_registry::docker_image_info(&tag).is_err() {
            progress("正在下载依赖并构建共享运行时，首次准备可能需要数分钟");
            let context = self.runtime_directory()?.join(format!("build-{}", id()));
            fs::create_dir(&context).map_err(storage)?;
            let result = (|| {
                for (name, body) in files {
                    let path = context.join(name);
                    fs::create_dir_all(path.parent().unwrap()).map_err(storage)?;
                    fs::write(path, body).map_err(storage)?;
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
                )
                .map_err(|_| {
                    err(
                        "runtime_preparation_failed",
                        "运行时构建超时或无法启动，请检查 Docker 和网络后重试",
                    )
                })?;
                require(
                    output.code == 0,
                    "运行时构建失败；请检查 Docker 和镜像/npm 网络访问后重试",
                )
            })();
            // Only this call's generated build context is removed, never a user's directory.
            let _ = fs::remove_dir_all(context);
            result?;
        }
        progress("正在核验运行时镜像内容与平台");
        self.register_docker_runtime(provider, &tag, version, license)
    }

    fn runtime_directory(&self) -> Result<PathBuf> {
        let path = Path::new(&self.root()).join("runtimes");
        reject_symlink(&path)?;
        fs::create_dir_all(&path).map_err(storage)?;
        Ok(path)
    }

    /// Authentication needs a host binary, but never the user's desktop CODEX_HOME.
    pub fn prepare_codex_auth(&self, progress: impl Fn(&str)) -> Result<RuntimeInstallation> {
        progress("正在准备 Codex 登录组件");
        if let Some(runtime) = self.runtime_installations()?.into_iter().find(|r| {
            r.key.provider == HarnessProvider::Codex
                && r.key.version == CODEX_VERSION
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
        if let Ok(runtime) = self.discover_codex() {
            if runtime.key.version == CODEX_VERSION {
                return Ok(runtime);
            }
        }
        let (target, checksum) = native_release(std::env::consts::OS, std::env::consts::ARCH)?;
        let directory = self
            .runtime_directory()?
            .join(format!("codex-{CODEX_VERSION}-{target}"));
        reject_symlink(&directory)?;
        fs::create_dir_all(&directory).map_err(storage)?;
        let binary = directory.join(format!("codex-{target}"));
        reject_symlink(&binary)?;
        {
            progress("正在下载并校验 Codex 官方登录组件");
            let staging = self.runtime_directory()?.join(format!("download-{}", id()));
            fs::create_dir(&staging).map_err(storage)?;
            let result = (|| {
                let archive = staging.join("codex.tar.gz");
                let url = format!(
                    "https://github.com/openai/codex/releases/download/rust-v{CODEX_VERSION}/codex-{target}.tar.gz"
                );
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
                    runtime_registry::file_digest(&archive)? == checksum,
                    "Codex 官方安装包校验失败",
                )?;
                let mut command = Command::new("tar");
                command
                    .arg("-xzf")
                    .arg(&archive)
                    .arg("-C")
                    .arg(&staging)
                    .arg(format!("codex-{target}"));
                let unpacked = process::capture_command(command, Duration::from_secs(30), None)?;
                require(unpacked.code == 0, "Codex 登录组件解包失败")?;
                fs::rename(staging.join(format!("codex-{target}")), &binary).map_err(storage)
            })();
            let _ = fs::remove_dir_all(staging);
            result?;
        }
        self.register_runtime(RuntimeRegistration {
            key: RuntimeKey {
                provider: HarnessProvider::Codex,
                version: CODEX_VERSION.into(),
                os: std::env::consts::OS.into(),
                arch: std::env::consts::ARCH.into(),
            },
            artifact: RuntimeArtifact::Executable {
                canonical_path: binary.to_string_lossy().into_owned(),
                sha256: runtime_registry::file_digest(&binary)?,
            },
            source: format!("https://github.com/openai/codex/releases/tag/rust-v{CODEX_VERSION}"),
            license: "Apache-2.0".into(),
        })
    }
}

fn native_release(os: &str, arch: &str) -> Result<(&'static str, &'static str)> {
    // SHA256 values published by the pinned official GitHub release's asset metadata.
    Ok(match (os, arch) {
        ("macos", "aarch64") => (
            "aarch64-apple-darwin",
            "4b22a2a971be2791c3e31f912f3e77a88ebd64a65812a7289ccba65dd6425fe9",
        ),
        ("macos", "x86_64") => (
            "x86_64-apple-darwin",
            "d8eb4a865a0b17319cde10347523e5bab0978d01725b39f4a639f40905e01221",
        ),
        ("linux", "aarch64") => (
            "aarch64-unknown-linux-musl",
            "843489acb4d4b1079745c36b03c5963b887b83155af208fd53584e33ee8eb281",
        ),
        ("linux", "x86_64") => (
            "x86_64-unknown-linux-musl",
            "9d64836106d5b374e90ee4454c7ebcde7ca7106558bd7e41a34b9f26ebda251a",
        ),
        _ => return Err(err("unsupported", "当前平台尚无受支持的 Codex 登录组件")),
    })
}
