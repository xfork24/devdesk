//! Serialize preparation per provider without holding database or project-selection locks.
use super::*;
use std::collections::BTreeMap;
use std::sync::RwLock;

#[derive(Default)]
pub struct RuntimePreparation {
    codex: Mutex<()>,
    dsh: Mutex<()>,
    auth: Mutex<()>,
    status: RwLock<BTreeMap<String, Value>>,
}
impl RuntimePreparation {
    pub fn run<T>(&self, key: &str, work: impl FnOnce(&dyn Fn(&str)) -> Result<T>) -> Result<T> {
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
    pub fn status(&self) -> Value {
        json!({"preparations":self.status.read().unwrap().values().collect::<Vec<_>>()})
    }
}

pub struct SessionResourceAdapter<'a> {
    pub global: &'a GlobalStore,
    pub codex_auth: &'a codex_auth::CodexAuthentication,
    pub preparation: &'a RuntimePreparation,
}
impl RuntimePreparation {
    pub fn prepare_auth(&self, global: &GlobalStore) -> Result<RuntimeInstallation> {
        self.run("codex-auth", |progress| global.prepare_codex_auth(progress))
    }
}
impl ports::SessionResources for SessionResourceAdapter<'_> {
    fn shared_store(&self) -> &dyn EngineeringStore {
        self.global
    }
    fn architecture(&self) -> String {
        std::env::consts::ARCH.into()
    }
    fn prepare_harness(
        &self,
        record: &ConfigurationRecord,
        resource: &ConfigurationRecord,
    ) -> Result<RuntimeInstallation> {
        let harness: HarnessDefinition = record.decode()?;
        let selected: RuntimeResourceDefinition = resource.decode()?;
        require(
            selected.provider.as_str() == harness.provider,
            "运行资源的协议驱动不匹配",
        )?;
        self.preparation.run(harness.provider.as_str(), |progress| {
            self.global.prepare_runtime_resource(resource, progress)
        })
    }
    fn installations(&self) -> Result<Vec<RuntimeInstallation>> {
        self.global.runtime_installations()
    }
    fn ensure_runtime(
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
                "所选运行时镜像不可用；固定安装不会自动替换",
            )?;
            return Ok(runtime);
        }
        self.preparation.run(provider.as_str(), |progress| {
            self.global.prepare_docker_runtime(provider, progress)
        })
    }
    fn runtime_verified(&self, id: &str) -> Result<bool> {
        Ok(self.global.runtime_integrity(id)?.verified)
    }
    fn installation_recipe(&self, i: &RuntimeInstallation) -> String {
        permissions::installation_recipe(i)
    }
    fn validate_installation(&self, i: &RuntimeInstallation) -> Result<()> {
        permissions::validate_runtime_version(i)?;
        self.global.require_runtime_verification(i)
    }
    fn verify_permissions(
        &self,
        s: &EngineeringState,
        l: &VersionedPermissionLock,
        i: &RuntimeInstallation,
    ) -> Result<()> {
        permission_profiles::verify_permission_installation(s, l, i)
    }
    fn compile_permissions(
        &self,
        s: &EngineeringState,
        l: &VersionedPermissionLock,
        scope: Option<ExecutionScope>,
    ) -> Result<ExecutionPermissions> {
        permission_profiles::compile_permission_profiles(s, l, scope)
    }
    fn adapter_identity(&self, p: HarnessProvider) -> String {
        harness::managed_adapter_identity(p)
    }
    fn credential_reference(
        &self,
        provider: HarnessProvider,
        connection: Option<&ModelConnection>,
        state: &EngineeringState,
    ) -> Result<Option<String>> {
        match provider {
            HarnessProvider::Codex => Ok(Some(self.codex_auth.credential_reference()?)),
            HarnessProvider::Dsh => {
                let c = connection.ok_or_else(|| err("invalid_configuration", "缺少模型连接"))?;
                let facts = state.provider_facts_any(&c.provider)?;
                if !facts.requires_key {
                    return Ok(None);
                }
                let vault = credentials::CredentialVault::new(Path::new(&self.global.root()))?;
                let status = vault.status_model_reference(
                    &facts,
                    self.global.model_credential_reference(&facts)?.as_deref(),
                )?;
                require(
                    status.available,
                    "系统凭据库不可用，请检查共享凭据助手或 Secret Service",
                )?;
                require(status.configured, format!("请先配置 {} 凭据", facts.label))?;
                vault.verify_model_reference(&facts, &status.reference)?;
                Ok(Some(status.reference))
            }
        }
    }
    fn model_profile_digest(&self, c: &ModelConnection, m: &str) -> Result<String> {
        model_providers::dsh_model_patch(c, m).and_then(|p| configuration_digest(&json!(p)))
    }
    fn runtime_digest(&self, a: &Definition, b: &RuntimeBinding) -> Result<String> {
        runtime_digest(a, b)
    }
}
