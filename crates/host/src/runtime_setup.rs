//! Serialize preparation per provider without holding database or project-selection locks.
use super::*;
use std::sync::RwLock;

#[derive(Default)]
pub(super) struct RuntimeSetup {
    codex: Mutex<()>,
    dsh: Mutex<()>,
    auth: Mutex<()>,
    status: RwLock<BTreeMap<String, Value>>,
}
impl RuntimeSetup {
    fn run<T>(&self, key: &str, work: impl FnOnce(&dyn Fn(&str)) -> Result<T>) -> Result<T> {
        let _guard = match key {
            "codex" => &self.codex,
            "dsh" => &self.dsh,
            _ => &self.auth,
        }
        .lock()
        .map_err(|e| err("storage", e.to_string()))?;
        let progress = |message: &str| {
            self.status.write().unwrap().insert(
                key.into(),
                json!({"provider":key,"status":"preparing","message":message}),
            );
        };
        progress("正在检查运行环境");
        let result = work(&progress);
        self.status.write().unwrap().insert(key.into(), match &result {
            Ok(_) => json!({"provider":key,"status":"ready","message":"运行环境已就绪"}),
            Err(error) => json!({"provider":key,"status":"failed","message":error.message,"code":error.code}),
        });
        result
    }
    pub(super) fn status(&self) -> Value {
        json!({"preparations":self.status.read().unwrap().values().collect::<Vec<_>>()})
    }
}
impl Workspace {
    pub(super) fn ensure_runtime(
        &self,
        provider: HarnessProvider,
        preferred: Option<&str>,
    ) -> Result<RuntimeInstallation> {
        if let Some(id) = preferred {
            let runtime = self
                .global
                .runtime_installations()?
                .into_iter()
                .find(|r| {
                    r.id == id
                        && r.key.provider == provider
                        && r.key.os == "linux"
                        && matches!(r.artifact, RuntimeArtifact::DockerImage { .. })
                })
                .ok_or_else(|| err("blocked", "所选 Harness 安装不存在或平台不兼容"))?;
            require(
                self.global.runtime_integrity(id)?.verified,
                "所选运行时镜像不可用；固定安装不会自动替换，请检查 Docker 或重新选择安装",
            )?;
            return Ok(runtime);
        }
        self.runtime_setup.run(provider.as_str(), |progress| {
            self.global.prepare_docker_runtime(provider, progress)
        })
    }
    pub(super) fn ensure_codex_auth(&self) -> Result<RuntimeInstallation> {
        self.runtime_setup.run("codex-auth", |progress| {
            self.global.prepare_codex_auth(progress)
        })
    }
}
