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

impl Workspace {
    /// Bind a managed Harness revision to its first verified image on this platform.
    pub(super) fn materialize_harness(
        &self,
        e: &Engine,
        record: &ConfigurationRecord,
    ) -> Result<RuntimeInstallation> {
        let harness: HarnessDefinition = record.decode()?;
        let digest = harness
            .recipe_digest
            .as_ref()
            .ok_or_else(|| err("invalid_configuration", "Harness 没有固定构建配方"))?;
        let store: Arc<dyn EngineeringStore> = if record.scope == Scope::Global {
            self.global.clone()
        } else {
            e.store.clone()
        };
        let key = format!(
            "harness-materialization:{}:linux:{}",
            record.id,
            std::env::consts::ARCH
        );
        if let Some(saved) = store.snapshot()?.governance.iter().find(|r| r.id == key) {
            require(
                saved.data["recipe_digest"] == *digest,
                "Harness 配方与物化回执不一致",
            )?;
            let installation = saved.data["installation_id"]
                .as_str()
                .ok_or_else(|| err("storage", "物化回执缺少安装"))?;
            return self.ensure_runtime(HarnessProvider::Codex, Some(installation));
        }
        require(
            harness.provider == "codex"
                && harness.runtime_version == DEFAULT_CODEX_VERSION
                && *digest == default_codex_recipe_digest(),
            "当前程序无法构建此冻结配方；不能静默换版",
        )?;
        let installation = self.ensure_runtime(HarnessProvider::Codex, None)?;
        require(
            installation
                .key
                .version
                .starts_with(&format!("{}+docker.", harness.runtime_version)),
            "实际安装与冻结 Codex 版本不符",
        )?;
        let data = json!({"harness_revision_id":record.id,"recipe_digest":digest,"installation_id":installation.id,"key":installation.key,"artifact":installation.artifact});
        store.transact(None, &mut |s| {
            if let Some(previous) = s.governance.iter().find(|r| r.id == key) {
                require(
                    previous.data == data,
                    "此 Harness 已物化为另一个安装，禁止替换",
                )?;
            } else {
                s.governance.push(ConfigurationRecord {
                    id: key.clone(),
                    kind: "harness_materialization".into(),
                    definition_id: record.id.clone(),
                    scope: record.scope.clone(),
                    name: format!("{} · 已核验安装", record.name),
                    version: 1,
                    supersedes: None,
                    content_hash: configuration_hash(&data),
                    data: data.clone(),
                    immutable: true,
                    revision: 1,
                    created_at: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as u64,
                });
            }
            Ok(Value::Null)
        })?;
        Ok(installation)
    }
}
